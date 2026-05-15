"""Configuration for the Cookest image generation service."""
import os
from dataclasses import dataclass, field


@dataclass
class Config:
    host: str = field(default_factory=lambda: os.getenv("HOST", "0.0.0.0"))
    port: int = field(default_factory=lambda: int(os.getenv("PORT", "8082")))

    # SD model — swap for SDXL or anything on HuggingFace
    model_id: str = field(
        default_factory=lambda: os.getenv(
            "SD_MODEL_ID", "runwayml/stable-diffusion-v1-5"
        )
    )
    model_cache_dir: str = field(
        default_factory=lambda: os.getenv("MODEL_CACHE_DIR", "./model_cache")
    )
    generated_dir: str = field(
        default_factory=lambda: os.getenv("GENERATED_DIR", "./generated")
    )

    # Image settings
    image_width: int = field(default_factory=lambda: int(os.getenv("IMG_WIDTH", "512")))
    image_height: int = field(default_factory=lambda: int(os.getenv("IMG_HEIGHT", "512")))
    num_inference_steps: int = field(
        default_factory=lambda: int(os.getenv("INFERENCE_STEPS", "25"))
    )
    guidance_scale: float = field(
        default_factory=lambda: float(os.getenv("GUIDANCE_SCALE", "7.5"))
    )

    # Queue settings
    max_queue_size: int = field(
        default_factory=lambda: int(os.getenv("MAX_QUEUE_SIZE", "50"))
    )
    # Number of parallel workers (usually 1 for CPU, can be >1 with GPU)
    num_workers: int = field(
        default_factory=lambda: int(os.getenv("NUM_WORKERS", "1"))
    )

    # Base URL used to build public image URLs returned to clients
    public_base_url: str = field(
        default_factory=lambda: os.getenv("PUBLIC_BASE_URL", "http://localhost:8082")
    )

    # Optional: request auth token (app-api should pass this header)
    internal_token: str = field(
        default_factory=lambda: os.getenv("IMAGE_GEN_TOKEN", "")
    )

    # Device: "cpu", "cuda", "mps"
    device: str = field(default_factory=lambda: os.getenv("TORCH_DEVICE", "cpu"))


cfg = Config()
