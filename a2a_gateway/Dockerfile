FROM python:3.12-slim-bookworm

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
