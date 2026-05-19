FROM python:3.12-slim

ENV PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_NO_CACHE_DIR=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

ARG MINICONF_PY_EXTRAS=

RUN apt-get update \
    && apt-get install -y --no-install-recommends bash ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/miniconf-py

COPY pyproject.toml README.md ./
COPY miniconf ./miniconf

RUN python -m pip install --upgrade pip \
    && if [ -n "$MINICONF_PY_EXTRAS" ]; then \
        python -m pip install ".[${MINICONF_PY_EXTRAS}]"; \
    else \
        python -m pip install .; \
    fi

WORKDIR /work

CMD ["python", "-m", "miniconf", "--help"]
