FROM rust:1.69 as builder
WORKDIR /usr/src/sexurity

COPY . .
RUN cargo install --path src/sexurity-discord
RUN cargo install --path src/sexurity-poller

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y wget jq && rm -rf /var/lib/apt/lists/*
RUN wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64 && chmod +x /usr/local/bin/yq

COPY --from=builder /usr/local/cargo/bin/sexurity-discord /usr/local/bin/sexurity-discord
COPY --from=builder /usr/local/cargo/bin/sexurity-poller /usr/local/bin/sexurity-poller
COPY ./yaml_to_cli.sh /usr/local/bin/yaml_to_cli.sh
ENV RUST_TRACE info