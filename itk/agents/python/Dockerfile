FROM python:3.12-slim@sha256:a6e34c598f2467ed0e9a8d349809fcd8b5c603269512df273a0bb1784edc11b1
WORKDIR /app
# requirements.lock is requirements.txt hash-locked (uv pip compile
# --generate-hashes; see .github/requirements/README.md).
COPY requirements.lock .
RUN pip install --no-cache-dir --require-hashes -r requirements.lock
COPY agent.py .
EXPOSE 9100
CMD ["python", "agent.py", "--port", "9100"]
