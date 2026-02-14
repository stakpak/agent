# =============================================================================
# Stakpak Agent - Lean Docker Image with On-Demand Tool Installation
# =============================================================================
#
# Uses aqua for lazy-loading CLI tools, reducing image size from ~2GB to ~600MB.
# Tools are downloaded on first use and can be cached via Docker volumes.
#
# CACHING:
# --------
# Mount a volume to persist downloaded tools across container runs:
#
#   docker run -v stakpak-cache:/home/agent/.local/share/aquaproj-aqua stakpak/agent
#
# Pre-warm cache with all tools:
#
#   docker run -v stakpak-cache:/home/agent/.local/share/aquaproj-aqua stakpak/agent \
#     sh -c "kubectl version --client && terraform version && helm version && \
#            aws --version && doctl version && gcloud --version && az --version"
#
# =============================================================================

FROM rust:1.91.0-slim-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release --target-dir /usr/src/app/target
RUN strip /usr/src/app/target/release/stakpak

FROM python:3.13-slim-bookworm
LABEL org.opencontainers.image.source="https://github.com/stakpak/agent" \
    org.opencontainers.image.description="Stakpak Agent" \
    maintainer="contact@stakpak.dev"

# Install basic dependencies
RUN apt-get update -y && apt-get install -y \
    curl \
    unzip \
    git \
    apt-transport-https \
    ca-certificates \
    gnupg \
    netcat-traditional \
    wget \
    dnsutils \
    sudo \
    && rm -rf /var/lib/apt/lists/*

# Install Docker CLI
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
    && chmod a+r /etc/apt/keyrings/docker.gpg \
    && echo \
    "deb [arch="$(dpkg --print-architecture)" signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian \
    "$(. /etc/os-release && echo "$VERSION_CODENAME")" stable" | \
    tee /etc/apt/sources.list.d/docker.list > /dev/null \
    && apt-get update \
    && apt-get install -y docker-ce-cli \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/stakpak /usr/local/bin
RUN chmod +x /usr/local/bin/stakpak

# Create agent user and group
RUN groupadd -r agent && useradd -r -g agent -s /bin/bash -m agent && mkdir -p /agent && chown -R agent:agent /agent
# Create docker group and add agent user to it
RUN groupadd -r docker && usermod -aG docker agent

# Configure sudo to allow package management
RUN echo "# Allow agent user to manage packages" > /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) NOPASSWD: /usr/bin/apt-get, /usr/bin/apt, /usr/bin/dpkg, /usr/bin/snap" >> /etc/sudoers.d/agent && \
    echo 'agent ALL=(ALL) NOPASSWD: /usr/bin/mkdir -p /opt/*, /usr/bin/chown -R agent\:agent /opt/*, /usr/bin/tar -xzf * -C /opt' >> /etc/sudoers.d/agent && \
    chmod 440 /etc/sudoers.d/agent

# Create directories for cloud CLIs (installed on-demand)
RUN mkdir -p /opt/google-cloud-sdk /opt/azure-cli \
    && chown -R agent:agent /opt/google-cloud-sdk /opt/azure-cli

# Install aqua (lazy-loading CLI tool manager)
USER agent
WORKDIR /home/agent

RUN curl -sSfL -o aqua-installer https://raw.githubusercontent.com/aquaproj/aqua-installer/v4.0.4/aqua-installer \
    && echo "acd21cbb06609dd9a701b0032ba4c21fa37b0e3b5cc4c9d721cc02f25ea33a28  aqua-installer" | sha256sum -c - \
    && chmod +x aqua-installer \
    && ./aqua-installer -v v2.56.6 \
    && rm aqua-installer

ENV AQUA_ROOT_DIR=/home/agent/.local/share/aquaproj-aqua
ENV AQUA_GLOBAL_CONFIG=/home/agent/.config/aquaproj-aqua/aqua.yaml
ENV PATH="${AQUA_ROOT_DIR}/bin:/home/agent/.local/bin:${PATH}"

# Configure aqua and wrapper scripts
RUN mkdir -p /home/agent/.local/bin

COPY --chown=agent:agent aqua.yaml /home/agent/.config/aquaproj-aqua/aqua.yaml
COPY --chown=agent:agent scripts/gcloud-wrapper.sh /home/agent/.local/bin/gcloud
COPY --chown=agent:agent scripts/gsutil-wrapper.sh /home/agent/.local/bin/gsutil
COPY --chown=agent:agent scripts/bq-wrapper.sh /home/agent/.local/bin/bq
COPY --chown=agent:agent scripts/az-wrapper.sh /home/agent/.local/bin/az

RUN chmod +x /home/agent/.local/bin/*

# Initialize aqua (creates symlinks, doesn't download tools)
RUN aqua install -a --only-link

WORKDIR /agent/

USER agent

ENTRYPOINT ["/usr/local/bin/stakpak"]
