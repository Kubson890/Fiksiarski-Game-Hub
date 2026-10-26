FROM python:3.12-slim

WORKDIR /app

RUN pip install --no-cache-dir fastapi "uvicorn[standard]" pydantic python-multipart websockets bcrypt

COPY server.py /app/server.py

ENV DATA_DIR=/app/data
ENV SERVER_PORT=47832
VOLUME ["/app/data"]

EXPOSE 47832

CMD ["uvicorn", "server:app", "--host", "0.0.0.0", "--port", "47832"]