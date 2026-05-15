FROM python:3.11-slim

WORKDIR /app

# System dependencies for PyTorch + Pillow
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgl1 \
    libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

# Install Python deps (CPU torch by default)
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# App code
COPY . .

# Pre-create directories
RUN mkdir -p /app/generated /app/model_cache

# Expose service port
EXPOSE 8082

# Run
CMD ["python", "run.py"]
