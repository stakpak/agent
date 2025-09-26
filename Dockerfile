FROM rust:1.89.0-slim-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release --target-dir /usr/src/app/target
RUN strip /usr/src/app/target/release/stakpak

FROM debian:bookworm-slim
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
    jq \
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

# Install aws cli
RUN cd /tmp && \
    ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "aarch64" ]; then \
    curl "https://awscli.amazonaws.com/awscli-exe-linux-$ARCH.zip" -o "awscliv2.zip"; \
    else \
    echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    unzip awscliv2.zip && \
    ./aws/install && \
    rm -rf aws awscliv2.zip

# Install digital ocean cli
RUN cd /tmp && \
    ARCH=$(uname -m) && \
    DOCTL_VERSION=$(curl -s https://api.github.com/repos/digitalocean/doctl/releases/latest | jq -r '.tag_name' | sed 's/^v//') && \
    if [ "$ARCH" = "x86_64" ]; then \
    DOCTL_ARCH="amd64"; \
    elif [ "$ARCH" = "aarch64" ]; then \
    DOCTL_ARCH="arm64"; \
    else \
    echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    curl -LO "https://github.com/digitalocean/doctl/releases/download/v${DOCTL_VERSION}/doctl-${DOCTL_VERSION}-linux-${DOCTL_ARCH}.tar.gz" && \
    tar xf "doctl-${DOCTL_VERSION}-linux-${DOCTL_ARCH}.tar.gz" && \
    mv doctl /usr/local/bin && \
    rm "doctl-${DOCTL_VERSION}-linux-${DOCTL_ARCH}.tar.gz"

# Install gcloud cli
RUN echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main" | tee -a /etc/apt/sources.list.d/google-cloud-sdk.list && \
    curl https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg && \
    apt-get update -y && \
    apt-get install google-cloud-cli -y

# Install azure cli
RUN curl -sL https://aka.ms/InstallAzureCLIDeb | bash

# Install terraform cli
RUN cd /tmp && \
    ARCH=$(uname -m) && \
    TERRAFORM_VERSION=$(curl -s https://api.releases.hashicorp.com/v1/releases/terraform | jq -r '.[0].version') && \
    if [ "$ARCH" = "x86_64" ]; then \
    TERRAFORM_ARCH="amd64"; \
    elif [ "$ARCH" = "aarch64" ]; then \
    TERRAFORM_ARCH="arm64"; \
    else \
    echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    curl -LO "https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_linux_${TERRAFORM_ARCH}.zip" && \
    unzip "terraform_${TERRAFORM_VERSION}_linux_${TERRAFORM_ARCH}.zip" && \
    mv terraform /usr/local/bin && \
    rm "terraform_${TERRAFORM_VERSION}_linux_${TERRAFORM_ARCH}.zip"


COPY --from=builder /usr/src/app/target/release/stakpak /usr/local/bin
RUN chmod +x /usr/local/bin/stakpak

# Create agent user and group
RUN groupadd -r agent && useradd -r -g agent -s /bin/bash -m agent && mkdir -p /agent && chown -R agent:agent /agent
# Create docker group and add agent user to it
RUN groupadd -r docker && usermod -aG docker agent


# Configure sudo to allow package management
RUN echo "# Allow agent user to manage packages" > /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) NOPASSWD: /usr/bin/apt-get, /usr/bin/apt, /usr/bin/dpkg, /usr/bin/snap" >> /etc/sudoers.d/agent && \
    chmod 440 /etc/sudoers.d/agent


WORKDIR /agent/

USER agent

ENTRYPOINT ["/usr/local/bin/stakpak"]