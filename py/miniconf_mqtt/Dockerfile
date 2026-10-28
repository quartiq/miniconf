FROM python:3.12-slim

ENV PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_NO_CACHE_DIR=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

RUN apt-get update \
    && apt-get install -y --no-install-recommends bash ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/miniconf-py

COPY pyproject.toml README.md LICENSE-MIT ./
COPY src ./src

RUN python -m pip install --upgrade pip \
    && python -m pip install .

WORKDIR /work

CMD ["python", "-m", "miniconf_mqtt", "--help"]
