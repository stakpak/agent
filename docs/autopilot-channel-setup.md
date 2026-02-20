# Autopilot Channel Setup

## Telegram

1. Message [@BotFather](https://t.me/BotFather) → `/newbot` → copy the bot token
2. DM your bot so it can receive messages, then fetch your chat ID:
   ```
   curl -s "https://api.telegram.org/bot<TOKEN>/getUpdates" | jq '.result[0].message.chat.id'
   ```
3. Add the channel:
   ```
   stakpak autopilot channel add telegram --token "<TOKEN>"
   ```
4. Start: `stakpak --profile <PROFILE> up`
5. Config (`~/.stakpak/autopilot.toml`):
   ```toml
   [channels.telegram]
   token = "<BOT_TOKEN>"
   require_mention = false
   ```

## Discord

1. Go to [Discord Developer Portal](https://discord.com/developers/applications) → New Application → Bot tab → copy token
2. Enable **Presence Intent**, **Server Members Intent**, and **Message Content Intent** in the Bot tab
3. Invite bot to your server:
   ```
   https://discord.com/oauth2/authorize?client_id=<APP_ID>&scope=bot&permissions=68608
   ```
4. Add the channel:
   ```
   stakpak autopilot channel add discord --token "<TOKEN>"
   ```
5. Start: `stakpak --profile <PROFILE> up`
6. Config (`~/.stakpak/autopilot.toml`):
   ```toml
   [channels.discord]
   token = "<BOT_TOKEN>"
   guilds = []
   ```

## Slack

1. Create a Slack app at [api.slack.com/apps](https://api.slack.com/apps) with Socket Mode enabled
2. **OAuth & Permissions** → add Bot Token Scopes:
   `app_mentions:read`, `channels:history`, `channels:read`, `chat:write`,
   `groups:history`, `groups:read`, `im:history`, `im:read`,
   `mpim:history`, `mpim:read`, `reactions:read`, `reactions:write`
3. **Event Subscriptions** → subscribe to bot events:
   `message.channels`, `message.groups`, `message.im`, `app_mention`
4. Generate an **App-Level Token** (scope: `connections:write`) and install the app to your workspace
5. Add the channel:
   ```
   stakpak autopilot channel add slack --bot-token "<BOT_TOKEN>" --app-token "<APP_TOKEN>"
   ```
6. Start: `stakpak --profile <PROFILE> up`
7. Config (`~/.stakpak/autopilot.toml`):
   ```toml
   [channels.slack]
   bot_token = "xoxb-..."
   app_token = "xapp-..."
   ```

## Verify

```bash
stakpak autopilot channel list       # check configured channels
stakpak autopilot channel test       # test connectivity
```
