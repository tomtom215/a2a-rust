FROM python:3.12-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY agent.py .
EXPOSE 9100
CMD ["python", "agent.py", "--port", "9100"]
