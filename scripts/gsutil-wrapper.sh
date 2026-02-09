#!/bin/bash
# gsutil wrapper (part of gcloud SDK)
/home/agent/.local/bin/gcloud --version > /dev/null 2>&1
exec /opt/google-cloud-sdk/bin/gsutil "$@"
