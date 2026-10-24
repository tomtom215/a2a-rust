FROM python:3.12-slim@sha256:a0f57064cbdcd6285fa45f51237ab0b35ea8ebaed06a1f5973e89e19dfb86704
WORKDIR /app
# requirements.lock is requirements.txt hash-locked (uv pip compile
# --generate-hashes; see .github/requirements/README.md).
COPY requirements.lock .
RUN pip install --no-cache-dir --require-hashes -r requirements.lock
COPY agent.py .
EXPOSE 9100
CMD ["python", "agent.py", "--port", "9100"]
