ARG BASE_IMAGE_TAG=latest

FROM --platform=linux/amd64 ghcr.io/stakpak/agent:${BASE_IMAGE_TAG}

# Update package list and install required packages in a single layer for optimization
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    openssl \
    redsocks \
    gettext-base \
    iptables \
    iproute2 \
    netcat-openbsd \
    net-tools \
    procps \
    sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# Create redsocks directory and copy configuration
RUN mkdir -p /etc/redsocks
COPY redsocks.conf /etc/redsocks/redsocks.conf.template

# Configure sudo to allow package management but deny network configuration
RUN echo "# Allow agent user to manage packages" > /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) NOPASSWD: /usr/bin/apt-get, /usr/bin/apt, /usr/bin/dpkg, /usr/bin/snap" >> /etc/sudoers.d/agent && \
    echo "# Explicitly deny network configuration commands" >> /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) !/usr/sbin/iptables*, !/sbin/iptables*, !/usr/bin/iptables*" >> /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) !/usr/sbin/ip6tables*, !/sbin/ip6tables*, !/usr/bin/ip6tables*" >> /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) !/usr/sbin/iptables-save, !/usr/sbin/iptables-restore" >> /etc/sudoers.d/agent && \
    echo "agent ALL=(ALL) !/usr/sbin/netfilter-persistent, !/usr/sbin/ufw" >> /etc/sudoers.d/agent && \
    chmod 440 /etc/sudoers.d/agent

# Create startup script for transparent proxy setup
COPY start-transparent-proxy.sh test-proxy.sh test-security.sh /usr/local/bin/
# Make scripts executable
RUN chmod +x /usr/local/bin/start-transparent-proxy.sh /usr/local/bin/test-proxy.sh /usr/local/bin/test-security.sh

# Ensure iptables and related tools are only accessible to root
# Remove setuid/setgid bits from network tools to prevent privilege escalation
RUN chmod u-s /usr/sbin/iptables* /usr/sbin/ip6tables* 2>/dev/null || true
RUN chmod u-s /sbin/iptables* /sbin/ip6tables* 2>/dev/null || true

# Set the entrypoint to start transparent proxy and then run the original command
ENTRYPOINT ["/usr/local/bin/start-transparent-proxy.sh"]