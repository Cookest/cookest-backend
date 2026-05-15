"""Image generation pipeline wrapper.

Wraps diffusers StableDiffusionPipeline in a thread-safe, lazy-loading class.
All heavy work runs in a thread-pool executor so FastAPI stays async.
"""
from __future__ import annotations

import asyncio
import logging
import os
import uuid
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from config import cfg

logger = logging.getLogger("image_gen.generator")

# Lazy-loaded pipeline (loaded once on first call)
_pipeline = None
_pipeline_lock = asyncio.Lock()
_executor = ThreadPoolExecutor(max_workers=cfg.num_workers, thread_name_prefix="sd-worker")


def _load_pipeline():
    """Load the Stable Diffusion pipeline (blocking — run in executor)."""
    import torch
    from diffusers import (
        StableDiffusionPipeline,
        DPMSolverMultistepScheduler,
    )

    logger.info("Loading SD model: %s on device: %s", cfg.model_id, cfg.device)

    pipe = StableDiffusionPipeline.from_pretrained(
        cfg.model_id,
        cache_dir=cfg.model_cache_dir,
        torch_dtype=torch.float16 if cfg.device in ("cuda", "mps") else torch.float32,
        safety_checker=None,        # disabled for food images — no NSFW concern
        requires_safety_checker=False,
    )

    # Use DPM-Solver++ for fast, high-quality sampling
    pipe.scheduler = DPMSolverMultistepScheduler.from_config(pipe.scheduler.config)
    pipe = pipe.to(cfg.device)

    if cfg.device == "cpu":
        # Memory-efficient settings for CPU inference
        pipe.enable_attention_slicing(1)
        try:
            pipe.enable_sequential_cpu_offload()
        except Exception:
            pass  # Not always available

    logger.info("SD pipeline ready.")
    return pipe


def _generate_image(positive: str, negative: str, seed: int | None = None) -> bytes:
    """Synchronous generate — returns raw PNG bytes."""
    import torch
    global _pipeline

    if _pipeline is None:
        _pipeline = _load_pipeline()

    generator = torch.Generator(device=cfg.device)
    if seed is not None:
        generator.manual_seed(seed)

    result = _pipeline(
        prompt=positive,
        negative_prompt=negative,
        width=cfg.image_width,
        height=cfg.image_height,
        num_inference_steps=cfg.num_inference_steps,
        guidance_scale=cfg.guidance_scale,
        generator=generator,
        num_images_per_prompt=1,
    )

    import io
    buf = io.BytesIO()
    result.images[0].save(buf, format="PNG")
    return buf.getvalue()


async def ensure_pipeline_loaded() -> None:
    """Pre-warm the pipeline asynchronously (call at startup)."""
    async with _pipeline_lock:
        if _pipeline is None:
            loop = asyncio.get_event_loop()
            await loop.run_in_executor(_executor, _load_pipeline)


async def generate(positive: str, negative: str, seed: int | None = None) -> str:
    """Generate an image asynchronously. Returns the public URL."""
    loop = asyncio.get_event_loop()
    png_bytes = await loop.run_in_executor(
        _executor, _generate_image, positive, negative, seed
    )

    # Save to disk
    Path(cfg.generated_dir).mkdir(parents=True, exist_ok=True)
    job_id = str(uuid.uuid4())
    file_path = Path(cfg.generated_dir) / f"{job_id}.png"
    file_path.write_bytes(png_bytes)

    public_url = f"{cfg.public_base_url.rstrip('/')}/images/{job_id}.png"
    logger.info("Image saved: %s → %s", file_path, public_url)
    return public_url
