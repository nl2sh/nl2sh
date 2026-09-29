# The base image is overridable so a host that builds through a Docker Hub mirror does not
# need to edit this file (Compose: NL2SH_GATEWAY_BASE_IMAGE; plain build: --build-arg
# GATEWAY_BASE_IMAGE=...). A mirror must serve the same Python tag as the default.
ARG GATEWAY_BASE_IMAGE=python:3.12-slim-bookworm
FROM ${GATEWAY_BASE_IMAGE}

RUN apt-get update \
    && apt-get install -y --no-install-recommends adb \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY pyproject.toml README.md ./
COPY nl2sh_a2a ./nl2sh_a2a
RUN pip install --no-cache-dir . \
    && useradd --create-home --uid 10001 gateway \
    && mkdir -p /data /home/gateway/.android \
    && chown -R gateway:gateway /data /home/gateway/.android

USER gateway
ENV PYTHONUNBUFFERED=1
EXPOSE 8765
ENTRYPOINT ["nl2sh-a2a"]