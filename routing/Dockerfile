FROM python:3.12-slim-bookworm
ENV PIP_DISABLE_PIP_VERSION_CHECK=1 PYTHONDONTWRITEBYTECODE=1 USE_TF=0 \
    HF_HOME=/models/huggingface TOKENIZERS_PARALLELISM=false \
    OMP_NUM_THREADS=2 LAYA_THREADS=2
WORKDIR /app
RUN pip install --no-cache-dir --no-deps torch==2.8.0 --index-url https://download.pytorch.org/whl/cpu
COPY requirements.txt requirements.lock ./
RUN pip install --no-cache-dir -r requirements.txt -c requirements.lock
COPY server.py routes.json gateway.py export_onnx.py ./
EXPOSE 8768
HEALTHCHECK --interval=30s --timeout=3s --start-period=300s --retries=5 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8768/health', timeout=2)"
CMD ["uvicorn", "server:app", "--host", "0.0.0.0", "--port", "8768", "--no-access-log"]
