use crate::{
    approval::ApprovalStateMachine,
    compaction::CompactionEngine,
    context::ContextReducer,
    error::AgentError,
    hooks::AgentHook,
    retry::exponential_backoff_ms,
    tools::{ToolExecutionResult, ToolExecutor},
    types::{
        AgentCommand, AgentConfig, AgentEvent, AgentLoopResult, AgentRunContext, ProposedToolCall,
        StopReason, ToolDecision, TurnFinishReason,
    },
};
use serde_json::json;
use stakai::{ContentPart, FinishReasonKind, Message, MessageContent, Role};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct RuntimeQueues {
    steering: VecDeque<String>,
    follow_up: VecDeque<String>,
    pending_tool_decisions: HashMap<String, ToolDecision>,
}

enum ToolCycleOutcome {
    Completed,
    Cancelled,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    run: AgentRunContext,
    inference: &stakai::Inference,
    config: &AgentConfig,
    mut initial_messages: Vec<Message>,
    context_metadata: &mut serde_json::Value,
    user_message: Message,
    tools: &dyn ToolExecutor,
    hooks: &[Box<dyn AgentHook>],
    event_tx: mpsc::Sender<AgentEvent>,
    mut command_rx: mpsc::Receiver<AgentCommand>,
    cancel: CancellationToken,
    compactor: &dyn CompactionEngine,
    context_reducer: &dyn ContextReducer,
) -> Result<AgentLoopResult, AgentError> {
    if !config.system_prompt.is_empty() && !has_system_message(&initial_messages) {
        initial_messages.insert(0, Message::new(Role::System, config.system_prompt.clone()));
    }

    let mut messages = initial_messages;
    messages.push(user_message);

    emit(&event_tx, AgentEvent::RunStarted { run_id: run.run_id }).await;

    if !context_metadata.is_object() {
        *context_metadata = serde_json::json!({});
    }

    let mut current_model = config.model.clone();
    let mut queues = RuntimeQueues::default();
    let mut total_usage = stakai::Usage::default();
    let mut total_turns = 0usize;

    'run_loop: loop {
        drain_runtime_commands_nonblocking(
            &mut command_rx,
            &mut queues,
            &mut current_model,
            &cancel,
        );

        if cancel.is_cancelled() {
            emit(
                &event_tx,
                AgentEvent::RunCompleted {
                    run_id: run.run_id,
                    total_turns,
                    total_usage: total_usage.clone(),
                    stop_reason: StopReason::Cancelled,
                },
            )
            .await;

            return Ok(AgentLoopResult {
                run_id: run.run_id,
                total_turns,
                total_usage,
                stop_reason: StopReason::Cancelled,
                messages,
                metadata: context_metadata.clone(),
            });
        }

        while let Some(steering) = queues.steering.pop_front() {
            if !steering.is_empty() {
                messages.push(Message::new(Role::User, steering));
            }
        }

        if total_turns >= config.max_turns {
            emit(
                &event_tx,
                AgentEvent::RunCompleted {
                    run_id: run.run_id,
                    total_turns,
                    total_usage: total_usage.clone(),
                    stop_reason: StopReason::MaxTurns,
                },
            )
            .await;

            return Ok(AgentLoopResult {
                run_id: run.run_id,
                total_turns,
                total_usage,
                stop_reason: StopReason::MaxTurns,
                messages,
                metadata: context_metadata.clone(),
            });
        }

        total_turns += 1;

        emit(
            &event_tx,
            AgentEvent::TurnStarted {
                run_id: run.run_id,
                turn: total_turns,
            },
        )
        .await;

        let mut attempt = 0usize;
        let response = loop {
            if cancel.is_cancelled() {
                emit(
                    &event_tx,
                    AgentEvent::RunCompleted {
                        run_id: run.run_id,
                        total_turns,
                        total_usage: total_usage.clone(),
                        stop_reason: StopReason::Cancelled,
                    },
                )
                .await;

                return Ok(AgentLoopResult {
                    run_id: run.run_id,
                    total_turns,
                    total_usage,
                    stop_reason: StopReason::Cancelled,
                    messages,
                    metadata: context_metadata.clone(),
                });
            }

            let reduced_messages = context_reducer.reduce(
                messages.clone(),
                &current_model,
                config.max_output_tokens,
                &config.tools,
                context_metadata,
            );

            for hook in hooks {
                hook.before_inference(&run, &reduced_messages, &current_model)
                    .await?;
            }

            let mut request = stakai::GenerateRequest::new(current_model.clone(), reduced_messages);
            request.provider_options = config.provider_options.clone();

            if config.max_output_tokens > 0 {
                request.options.max_tokens = Some(config.max_output_tokens);
            }

            if !config.tools.is_empty() {
                for tool in config.tools.iter().cloned() {
                    request.options = request.options.add_tool(tool);
                }
            }

            match inference.generate(&request).await {
                Ok(response) => {
                    break response;
                }
                Err(error) => {
                    let reason = error.to_string();
                    attempt += 1;

                    if config.compaction.enabled && is_context_overflow_error(&reason) {
                        emit(
                            &event_tx,
                            AgentEvent::CompactionStarted {
                                run_id: run.run_id,
                                reason: reason.clone(),
                            },
                        )
                        .await;

                        let compacted = compactor.compact(messages.clone(), &current_model).await?;
                        messages = compacted.messages;

                        emit(
                            &event_tx,
                            AgentEvent::CompactionCompleted {
                                run_id: run.run_id,
                                tokens_before: compacted.tokens_before,
                                tokens_after: compacted.tokens_after,
                                truncated: compacted.truncated,
                            },
                        )
                        .await;

                        total_turns = total_turns.saturating_sub(1);
                        continue 'run_loop;
                    }

                    if attempt < config.retry.max_attempts {
                        let delay_ms = exponential_backoff_ms(&config.retry, attempt);
                        emit(
                            &event_tx,
                            AgentEvent::RetryAttempt {
                                run_id: run.run_id,
                                attempt,
                                delay_ms,
                                reason: reason.clone(),
                            },
                        )
                        .await;

                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }

                    let agent_error = AgentError::Inference(reason.clone());

                    for hook in hooks {
                        let _ = hook.on_error(&run, &agent_error, &messages).await;
                    }

                    emit(
                        &event_tx,
                        AgentEvent::RunError {
                            run_id: run.run_id,
                            error: reason,
                            retryable: false,
                        },
                    )
                    .await;

                    return Err(agent_error);
                }
            }
        };

        add_usage(&mut total_usage, &response.usage);

        let mut assistant_parts: Vec<ContentPart> = Vec::new();
        let mut assistant_text = String::new();
        let mut thinking_text = String::new();
        let mut proposed_tool_calls: Vec<ProposedToolCall> = Vec::new();

        for content in &response.content {
            match content {
                stakai::ResponseContent::Text { text } => {
                    assistant_text.push_str(text);
                    assistant_parts.push(ContentPart::text(text.clone()));

                    emit(
                        &event_tx,
                        AgentEvent::TextDelta {
                            run_id: run.run_id,
                            delta: text.clone(),
                        },
                    )
                    .await;
                }
                stakai::ResponseContent::Reasoning { reasoning } => {
                    thinking_text.push_str(reasoning);
                }
                stakai::ResponseContent::ToolCall(tool_call) => {
                    assistant_parts.push(ContentPart::tool_call(
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        tool_call.arguments.clone(),
                    ));

                    proposed_tool_calls.push(ProposedToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        metadata: tool_call.metadata.clone(),
                    });
                }
            }
        }

        if !thinking_text.is_empty() {
            emit(
                &event_tx,
                AgentEvent::ThinkingDelta {
                    run_id: run.run_id,
                    delta: thinking_text,
                },
            )
            .await;
        }

        if !assistant_parts.is_empty() {
            messages.push(Message::new(
                Role::Assistant,
                MessageContent::Parts(assistant_parts),
            ));
        }

        for hook in hooks {
            hook.after_inference(&run, &messages, &current_model)
                .await?;
        }

        if !assistant_text.is_empty() {
            emit(
                &event_tx,
                AgentEvent::TextComplete {
                    run_id: run.run_id,
                    text: assistant_text,
                },
            )
            .await;
        }

        emit(
            &event_tx,
            AgentEvent::UsageReport {
                run_id: run.run_id,
                turn: total_turns,
                usage: response.usage.clone(),
            },
        )
        .await;

        if !proposed_tool_calls.is_empty() {
            emit(
                &event_tx,
                AgentEvent::ToolCallsProposed {
                    run_id: run.run_id,
                    tool_calls: proposed_tool_calls.clone(),
                },
            )
            .await;

            emit(
                &event_tx,
                AgentEvent::WaitingForToolApproval {
                    run_id: run.run_id,
                    pending_tool_call_ids: proposed_tool_calls
                        .iter()
                        .map(|tool_call| tool_call.id.clone())
                        .collect(),
                },
            )
            .await;

            let tool_outcome = run_tool_cycle(
                &run,
                config,
                tools,
                hooks,
                &event_tx,
                &mut command_rx,
                &cancel,
                &mut queues,
                &mut current_model,
                &mut messages,
                proposed_tool_calls,
            )
            .await?;

            match tool_outcome {
                ToolCycleOutcome::Cancelled => {
                    emit(
                        &event_tx,
                        AgentEvent::TurnCompleted {
                            run_id: run.run_id,
                            turn: total_turns,
                            finish_reason: TurnFinishReason::Cancelled,
                        },
                    )
                    .await;

                    emit(
                        &event_tx,
                        AgentEvent::RunCompleted {
                            run_id: run.run_id,
                            total_turns,
                            total_usage: total_usage.clone(),
                            stop_reason: StopReason::Cancelled,
                        },
                    )
                    .await;

                    return Ok(AgentLoopResult {
                        run_id: run.run_id,
                        total_turns,
                        total_usage,
                        stop_reason: StopReason::Cancelled,
                        messages,
                        metadata: context_metadata.clone(),
                    });
                }
                ToolCycleOutcome::Completed => {
                    emit(
                        &event_tx,
                        AgentEvent::TurnCompleted {
                            run_id: run.run_id,
                            turn: total_turns,
                            finish_reason: TurnFinishReason::ToolCalls,
                        },
                    )
                    .await;

                    continue;
                }
            }
        }

        let finish_reason = map_finish_reason(&response.finish_reason);

        emit(
            &event_tx,
            AgentEvent::TurnCompleted {
                run_id: run.run_id,
                turn: total_turns,
                finish_reason,
            },
        )
        .await;

        if let Some(follow_up) = queues.follow_up.pop_front()
            && !follow_up.is_empty()
        {
            messages.push(Message::new(Role::User, follow_up));
            continue;
        }

        let stop_reason = map_stop_reason(finish_reason);

        emit(
            &event_tx,
            AgentEvent::RunCompleted {
                run_id: run.run_id,
                total_turns,
                total_usage: total_usage.clone(),
                stop_reason,
            },
        )
        .await;

        return Ok(AgentLoopResult {
            run_id: run.run_id,
            total_turns,
            total_usage,
            stop_reason,
            messages,
            metadata: context_metadata.clone(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_tool_cycle(
    run: &AgentRunContext,
    config: &AgentConfig,
    tools: &dyn ToolExecutor,
    hooks: &[Box<dyn AgentHook>],
    event_tx: &mpsc::Sender<AgentEvent>,
    command_rx: &mut mpsc::Receiver<AgentCommand>,
    cancel: &CancellationToken,
    queues: &mut RuntimeQueues,
    current_model: &mut stakai::Model,
    messages: &mut Vec<Message>,
    proposed_tool_calls: Vec<ProposedToolCall>,
) -> Result<ToolCycleOutcome, AgentError> {
    let current_tool_ids: HashSet<String> = proposed_tool_calls
        .iter()
        .map(|tool_call| tool_call.id.clone())
        .collect();

    let mut approvals =
        ApprovalStateMachine::new(proposed_tool_calls.clone(), &config.tool_approval);

    let mut initial_decisions = HashMap::new();
    for tool_call_id in &current_tool_ids {
        if let Some(decision) = queues.pending_tool_decisions.remove(tool_call_id) {
            initial_decisions.insert(tool_call_id.clone(), decision);
        }
    }

    if !initial_decisions.is_empty() {
        approvals.apply_command(AgentCommand::ResolveTools {
            decisions: initial_decisions,
        })?;
    }

    let mut completed_tool_ids: HashSet<String> = HashSet::new();

    loop {
        if cancel.is_cancelled() {
            append_cancelled_placeholders(
                run,
                event_tx,
                messages,
                &proposed_tool_calls,
                &mut completed_tool_ids,
            )
            .await;
            return Ok(ToolCycleOutcome::Cancelled);
        }

        if !queues.steering.is_empty() {
            append_skipped_due_to_steering(
                run,
                event_tx,
                messages,
                &proposed_tool_calls,
                &mut completed_tool_ids,
            )
            .await;
            return Ok(ToolCycleOutcome::Completed);
        }

        if let Some(resolved) = approvals.next_ready() {
            let tool_call_id = resolved.tool_call.id.clone();
            let tool_name = resolved.tool_call.name.clone();

            match resolved.decision {
                ToolDecision::Accept => {
                    emit(
                        event_tx,
                        AgentEvent::ToolExecutionStarted {
                            run_id: run.run_id,
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                        },
                    )
                    .await;

                    for hook in hooks {
                        hook.before_tool_execution(run, &resolved.tool_call, messages)
                            .await?;
                    }

                    match tools
                        .execute_tool_call(run, &resolved.tool_call, cancel)
                        .await?
                    {
                        ToolExecutionResult::Cancelled => {
                            append_tool_result_message(
                                messages,
                                &tool_call_id,
                                json!({"error": "TOOL_CALL_CANCELLED"}),
                            );
                            completed_tool_ids.insert(tool_call_id.clone());

                            emit(
                                event_tx,
                                AgentEvent::ToolExecutionCompleted {
                                    run_id: run.run_id,
                                    tool_call_id,
                                    tool_name,
                                    result: "TOOL_CALL_CANCELLED".to_string(),
                                    is_error: true,
                                },
                            )
                            .await;

                            append_cancelled_placeholders(
                                run,
                                event_tx,
                                messages,
                                &proposed_tool_calls,
                                &mut completed_tool_ids,
                            )
                            .await;

                            return Ok(ToolCycleOutcome::Cancelled);
                        }
                        ToolExecutionResult::Completed { result, is_error } => {
                            append_tool_result_message(
                                messages,
                                &tool_call_id,
                                json!(result.clone()),
                            );
                            completed_tool_ids.insert(tool_call_id.clone());

                            emit(
                                event_tx,
                                AgentEvent::ToolExecutionCompleted {
                                    run_id: run.run_id,
                                    tool_call_id,
                                    tool_name,
                                    result,
                                    is_error,
                                },
                            )
                            .await;
                        }
                    }

                    for hook in hooks {
                        hook.after_tool_execution(run, &resolved.tool_call, messages)
                            .await?;
                    }
                }
                ToolDecision::Reject => {
                    let reason = "Tool call rejected by user".to_string();
                    append_tool_result_message(
                        messages,
                        &tool_call_id,
                        json!({"rejected": reason.clone()}),
                    );
                    completed_tool_ids.insert(tool_call_id.clone());

                    emit(
                        event_tx,
                        AgentEvent::ToolRejected {
                            run_id: run.run_id,
                            tool_call_id,
                            tool_name,
                            reason,
                        },
                    )
                    .await;
                }
                ToolDecision::CustomResult { content } => {
                    append_tool_result_message(messages, &tool_call_id, json!(content.clone()));
                    completed_tool_ids.insert(tool_call_id.clone());

                    emit(
                        event_tx,
                        AgentEvent::ToolExecutionCompleted {
                            run_id: run.run_id,
                            tool_call_id,
                            tool_name,
                            result: content,
                            is_error: false,
                        },
                    )
                    .await;
                }
            }

            drain_runtime_commands_nonblocking(command_rx, queues, current_model, cancel);
            continue;
        }

        if approvals.is_complete() {
            return Ok(ToolCycleOutcome::Completed);
        }

        let Some(command) = command_rx.recv().await else {
            continue;
        };

        match command {
            AgentCommand::ResolveTool {
                tool_call_id,
                decision,
            } => {
                if current_tool_ids.contains(&tool_call_id) {
                    approvals.apply_command(AgentCommand::ResolveTool {
                        tool_call_id,
                        decision,
                    })?;
                } else {
                    queues.pending_tool_decisions.insert(tool_call_id, decision);
                }
            }
            AgentCommand::ResolveTools { decisions } => {
                let mut apply_now = HashMap::new();
                for (tool_call_id, decision) in decisions {
                    if current_tool_ids.contains(&tool_call_id) {
                        apply_now.insert(tool_call_id, decision);
                    } else {
                        queues.pending_tool_decisions.insert(tool_call_id, decision);
                    }
                }

                if !apply_now.is_empty() {
                    approvals.apply_command(AgentCommand::ResolveTools {
                        decisions: apply_now,
                    })?;
                }
            }
            AgentCommand::Steering(text) => {
                queues.steering.push_back(text);
            }
            AgentCommand::FollowUp(text) => {
                queues.follow_up.push_back(text);
            }
            AgentCommand::SwitchModel(model) => {
                *current_model = model;
            }
            AgentCommand::Cancel => {
                cancel.cancel();
            }
        }
    }
}

fn append_tool_result_message(
    messages: &mut Vec<Message>,
    tool_call_id: &str,
    payload: serde_json::Value,
) {
    messages.push(Message::new(
        Role::Tool,
        MessageContent::Parts(vec![ContentPart::tool_result(
            tool_call_id.to_string(),
            payload,
        )]),
    ));
}

async fn append_cancelled_placeholders(
    run: &AgentRunContext,
    event_tx: &mpsc::Sender<AgentEvent>,
    messages: &mut Vec<Message>,
    proposed_tool_calls: &[ProposedToolCall],
    completed_tool_ids: &mut HashSet<String>,
) {
    for tool_call in proposed_tool_calls {
        if completed_tool_ids.contains(&tool_call.id) {
            continue;
        }

        completed_tool_ids.insert(tool_call.id.clone());
        append_tool_result_message(
            messages,
            &tool_call.id,
            json!({"error": "TOOL_CALL_CANCELLED"}),
        );

        emit(
            event_tx,
            AgentEvent::ToolExecutionCompleted {
                run_id: run.run_id,
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                result: "TOOL_CALL_CANCELLED".to_string(),
                is_error: true,
            },
        )
        .await;
    }
}

async fn append_skipped_due_to_steering(
    run: &AgentRunContext,
    event_tx: &mpsc::Sender<AgentEvent>,
    messages: &mut Vec<Message>,
    proposed_tool_calls: &[ProposedToolCall],
    completed_tool_ids: &mut HashSet<String>,
) {
    for tool_call in proposed_tool_calls {
        if completed_tool_ids.contains(&tool_call.id) {
            continue;
        }

        completed_tool_ids.insert(tool_call.id.clone());
        let reason = "Skipped due to steering update".to_string();

        append_tool_result_message(messages, &tool_call.id, json!({"skipped": reason.clone()}));

        emit(
            event_tx,
            AgentEvent::ToolRejected {
                run_id: run.run_id,
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                reason,
            },
        )
        .await;
    }
}

fn drain_runtime_commands_nonblocking(
    command_rx: &mut mpsc::Receiver<AgentCommand>,
    queues: &mut RuntimeQueues,
    current_model: &mut stakai::Model,
    cancel: &CancellationToken,
) {
    while let Ok(command) = command_rx.try_recv() {
        match command {
            AgentCommand::ResolveTool {
                tool_call_id,
                decision,
            } => {
                queues.pending_tool_decisions.insert(tool_call_id, decision);
            }
            AgentCommand::ResolveTools { decisions } => {
                for (tool_call_id, decision) in decisions {
                    queues.pending_tool_decisions.insert(tool_call_id, decision);
                }
            }
            AgentCommand::Steering(text) => {
                queues.steering.push_back(text);
            }
            AgentCommand::FollowUp(text) => {
                queues.follow_up.push_back(text);
            }
            AgentCommand::SwitchModel(model) => {
                *current_model = model;
            }
            AgentCommand::Cancel => cancel.cancel(),
        }
    }
}

fn map_finish_reason(reason: &stakai::FinishReason) -> TurnFinishReason {
    match reason.unified {
        FinishReasonKind::Stop => TurnFinishReason::Stop,
        FinishReasonKind::ToolCalls => TurnFinishReason::ToolCalls,
        FinishReasonKind::Length => TurnFinishReason::MaxOutputTokens,
        FinishReasonKind::Error => TurnFinishReason::Error,
        FinishReasonKind::ContentFilter | FinishReasonKind::Other => TurnFinishReason::Stop,
    }
}

fn map_stop_reason(reason: TurnFinishReason) -> StopReason {
    match reason {
        TurnFinishReason::Cancelled => StopReason::Cancelled,
        TurnFinishReason::Error => StopReason::Error,
        TurnFinishReason::Stop
        | TurnFinishReason::ToolCalls
        | TurnFinishReason::MaxOutputTokens => StopReason::Completed,
    }
}

fn add_usage(total: &mut stakai::Usage, usage: &stakai::Usage) {
    total.prompt_tokens = total.prompt_tokens.saturating_add(usage.prompt_tokens);
    total.completion_tokens = total
        .completion_tokens
        .saturating_add(usage.completion_tokens);
    total.total_tokens = total.total_tokens.saturating_add(usage.total_tokens);
}

fn has_system_message(messages: &[Message]) -> bool {
    messages.iter().any(|message| message.role == Role::System)
}

fn is_context_overflow_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    (error.contains("context") || error.contains("token"))
        && (error.contains("overflow")
            || error.contains("maximum")
            || error.contains("too long")
            || error.contains("limit"))
}

async fn emit(event_tx: &mpsc::Sender<AgentEvent>, event: AgentEvent) {
    let _ = event_tx.send(event).await;
}
