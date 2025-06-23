#!/bin/bash
set -e

# echo "Starting transparent proxy setup..."

# Install CA certificate if present
if [ -f "/tmp/warden_ca.crt" ]; then
    # echo "Installing CA certificate..."
    # Verify certificate format
    if ! openssl x509 -in /tmp/warden_ca.crt -text -noout > /dev/null 2>&1; then
        echo "ERROR: Invalid certificate format in warden_ca.crt" >&2
        exit 1
    fi
    # Copy certificate to the trusted certificate directory
    cp /tmp/warden_ca.crt /usr/local/share/ca-certificates/warden_ca.crt
    # Set proper permissions
    chmod 644 /usr/local/share/ca-certificates/warden_ca.crt
    # Update the certificate store
    update-ca-certificates 2>/dev/null || true
    # Verify certificate was installed successfully
    # if ! grep -q "warden_ca.crt" /etc/ssl/certs/ca-certificates.crt; then
    #     echo "WARNING: Certificate may not have been properly installed" >&2
    # fi
    #echo "Certificate installation completed successfully"
else
    echo "No CA certificate found at /tmp/warden_ca.crt, skipping certificate installation"
fi

# Apply environment variables to redsocks config
envsubst < /etc/redsocks/redsocks.conf.template > /etc/redsocks/redsocks.conf

# Test if redsocks config is valid
# echo "Testing redsocks configuration..."
redsocks -t -c /etc/redsocks/redsocks.conf
# echo "Redsocks configuration is valid"

# Test network connectivity to proxy
# echo "Testing connectivity to proxy server..."
if ! nc -z $WARDEN_PROXY_IP $WARDEN_PROXY_PORT 2>/dev/null; then
    echo "WARNING: Cannot connect to proxy server at $WARDEN_PROXY_IP:$WARDEN_PROXY_PORT"
    echo "Make sure your proxy server is running and accessible"
fi

# Start redsocks in background
# echo "Starting redsocks daemon..."
redsocks -c /etc/redsocks/redsocks.conf 2>/dev/null &
REDSOCKS_PID=$!

# Wait a moment for redsocks to start
sleep 2

# Verify redsocks is running
if ! kill -0 $REDSOCKS_PID 2>/dev/null; then
    echo "ERROR: Redsocks failed to start"
    exit 1
fi
# echo "Redsocks started successfully with PID: $REDSOCKS_PID"

# Create new iptables chain for redsocks
# echo "Setting up iptables rules..."
iptables -t nat -N REDSOCKS 2>/dev/null || true

# Flush existing rules in REDSOCKS chain
iptables -t nat -F REDSOCKS 2>/dev/null || true

# Ignore LANs and reserved addresses
# echo "Adding exclusions for local networks..."
iptables -t nat -A REDSOCKS -d 0.0.0.0/8 -j RETURN
iptables -t nat -A REDSOCKS -d 10.0.0.0/8 -j RETURN
iptables -t nat -A REDSOCKS -d 100.64.0.0/10 -j RETURN
iptables -t nat -A REDSOCKS -d 127.0.0.0/8 -j RETURN
iptables -t nat -A REDSOCKS -d 169.254.0.0/16 -j RETURN
iptables -t nat -A REDSOCKS -d 172.16.0.0/12 -j RETURN
iptables -t nat -A REDSOCKS -d 192.168.0.0/16 -j RETURN
iptables -t nat -A REDSOCKS -d 198.18.0.0/15 -j RETURN
iptables -t nat -A REDSOCKS -d 224.0.0.0/4 -j RETURN
iptables -t nat -A REDSOCKS -d 240.0.0.0/4 -j RETURN

# Redirect everything else to redsocks port
# echo "Adding redirect rule to redsocks port 12345..."
iptables -t nat -A REDSOCKS -p tcp -j REDIRECT --to-ports 12345

# Redirect all outbound TCP traffic through redsocks
# echo "Redirecting all outbound TCP traffic through redsocks..."
iptables -t nat -A OUTPUT -p tcp -j REDSOCKS

# echo "Transparent proxy setup completed successfully!"
# echo "Redsocks PID: $REDSOCKS_PID"

# Show current iptables rules for debugging
# echo "Current iptables NAT rules:"
# iptables -t nat -L REDSOCKS -n -v

# Function to cleanup on exit
cleanup() {
    # echo "Cleaning up..."
    iptables -t nat -D OUTPUT -p tcp -j REDSOCKS 2>/dev/null || true
    iptables -t nat -F REDSOCKS 2>/dev/null || true
    iptables -t nat -X REDSOCKS 2>/dev/null || true
    kill $REDSOCKS_PID 2>/dev/null || true
    # echo "Cleanup completed"
}

# Set trap for cleanup on exit
trap cleanup EXIT INT TERM

# If arguments provided, execute them; otherwise keep container running
if [ $# -gt 0 ]; then
    # echo "Executing command: $@"
    # Switch to non-root user to execute the command
    # This prevents the user from modifying iptables rules after setup
    exec runuser -u agent -- "$@"
else
    # Keep script running and forward signals to redsocks
    # echo "No command provided, keeping container running..."
    # Switch to non-root user for the waiting process
    runuser -u agent -c "sleep infinity" &
    USER_PID=$!
    
    # Wait for either redsocks or user process to exit
    while kill -0 $REDSOCKS_PID 2>/dev/null && kill -0 $USER_PID 2>/dev/null; do
        sleep 1
    done
fi