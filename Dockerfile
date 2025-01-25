FROM rust:1.82 as builder
WORKDIR /usr/src/security

COPY . .
RUN cargo install --path src/security_discord
RUN cargo install --path src/security_poller

FROM ubuntu
RUN apt-get update && apt-get install -y wget jq curl && rm -rf /var/lib/apt/lists/*
RUN wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64 && chmod +x /usr/local/bin/yq

COPY --from=builder /usr/local/cargo/bin/security_discord /usr/local/bin/discord
COPY --from=builder /usr/local/cargo/bin/security_poller /usr/local/bin/poller
COPY ./scripts/yaml_to_cli.sh /usr/local/bin/yaml_to_cli.sh
ENV RUST_LOG info
