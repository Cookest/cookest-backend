"""FastAPI application for Cookest image generation service.

Endpoints:
  GET  /health               — liveness probe
  POST /generate/step        — enqueue a recipe-step image generation job
  POST /generate/hero        — enqueue a hero/cover image generation job
  POST /generate/batch       — enqueue multiple step images at once
  GET  /jobs/{job_id}        — poll job status & get image URL when done
  GET  /jobs                 — list recent jobs (admin)
  GET  /images/{filename}    — serve a generated image

Auth (optional):
  If IMAGE_GEN_TOKEN env var is set, requests must include:
    Authorization: Bearer <token>
"""
from __future__ import annotations

import logging
import os
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, Depends, HTTPException, Request
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field
from typing import Optional

from config import cfg
from prompts import step_prompt, hero_prompt
from queue import queue, JobStatus

# ── Logging ───────────────────────────────────────────────────────────────────

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-8s  %(name)s  %(message)s",
)
logger = logging.getLogger("image_gen.api")


# ── App lifecycle ─────────────────────────────────────────────────────────────

@asynccontextmanager
async def lifespan(app: FastAPI):
    Path(cfg.generated_dir).mkdir(parents=True, exist_ok=True)
    Path(cfg.model_cache_dir).mkdir(parents=True, exist_ok=True)
    await queue.start()
    logger.info("Image generation service started on %s:%d", cfg.host, cfg.port)
    yield
    await queue.stop()
    logger.info("Image generation service stopped")


app = FastAPI(
    title="Cookest Image Generation API",
    version="1.0.0",
    lifespan=lifespan,
)

# Serve generated images as static files
app.mount(
    "/images",
    StaticFiles(directory=cfg.generated_dir, check_dir=False),
    name="images",
)


# ── Auth dependency ───────────────────────────────────────────────────────────

def verify_token(request: Request) -> None:
    """Optional internal token check. Skipped if IMAGE_GEN_TOKEN is empty."""
    if not cfg.internal_token:
        return
    auth = request.headers.get("Authorization", "")
    if not auth.startswith("Bearer ") or auth[7:] != cfg.internal_token:
        raise HTTPException(status_code=401, detail="Unauthorized")


# ── Pydantic models ───────────────────────────────────────────────────────────

class StepImageRequest(BaseModel):
    recipe_id: int
    recipe_name: str
    step_index: int
    total_steps: int
    step_description: str
    cuisine: Optional[str] = None
    seed: Optional[int] = None


class HeroImageRequest(BaseModel):
    recipe_id: int
    recipe_name: str
    description: Optional[str] = None
    cuisine: Optional[str] = None
    category: Optional[str] = None
    seed: Optional[int] = None


class BatchStepRequest(BaseModel):
    recipe_id: int
    recipe_name: str
    cuisine: Optional[str] = None
    steps: list[dict] = Field(..., description="List of {step_index, total_steps, step_description}")


class JobStatusResponse(BaseModel):
    job_id: str
    status: str
    image_url: Optional[str] = None
    error: Optional[str] = None
    metadata: dict = {}


# ── Routes ────────────────────────────────────────────────────────────────────

@app.get("/health")
async def health():
    return {"status": "ok", "model": cfg.model_id, "device": cfg.device}


@app.post("/generate/step", response_model=JobStatusResponse)
async def generate_step(
    req: StepImageRequest,
    _: None = Depends(verify_token),
):
    """Enqueue generation for one recipe step."""
    positive, negative = step_prompt(
        step_description=req.step_description,
        recipe_name=req.recipe_name,
        step_index=req.step_index,
        total_steps=req.total_steps,
        cuisine=req.cuisine,
    )
    job = await queue.submit(
        positive_prompt=positive,
        negative_prompt=negative,
        seed=req.seed,
        metadata={
            "recipe_id": req.recipe_id,
            "step_index": req.step_index,
            "type": "step",
        },
    )
    return JobStatusResponse(**job.to_dict())


@app.post("/generate/hero", response_model=JobStatusResponse)
async def generate_hero(
    req: HeroImageRequest,
    _: None = Depends(verify_token),
):
    """Enqueue generation for a recipe hero image."""
    positive, negative = hero_prompt(
        recipe_name=req.recipe_name,
        description=req.description,
        cuisine=req.cuisine,
        category=req.category,
    )
    job = await queue.submit(
        positive_prompt=positive,
        negative_prompt=negative,
        seed=req.seed,
        metadata={"recipe_id": req.recipe_id, "type": "hero"},
    )
    return JobStatusResponse(**job.to_dict())


@app.post("/generate/batch")
async def generate_batch(
    req: BatchStepRequest,
    _: None = Depends(verify_token),
):
    """Enqueue all steps of a recipe for image generation.
    Returns list of job IDs immediately; use /jobs/{id} to poll each one.
    """
    jobs = []
    total_steps = len(req.steps)
    for step in req.steps:
        positive, negative = step_prompt(
            step_description=step["step_description"],
            recipe_name=req.recipe_name,
            step_index=step["step_index"],
            total_steps=total_steps,
            cuisine=req.cuisine,
        )
        job = await queue.submit(
            positive_prompt=positive,
            negative_prompt=negative,
            metadata={
                "recipe_id": req.recipe_id,
                "step_index": step["step_index"],
                "type": "step",
            },
        )
        jobs.append({"step_index": step["step_index"], "job_id": job.id})
    return {"recipe_id": req.recipe_id, "jobs": jobs}


@app.get("/jobs/{job_id}", response_model=JobStatusResponse)
async def get_job(job_id: str, _: None = Depends(verify_token)):
    """Poll a specific job for its status and image URL."""
    job = queue.get(job_id)
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return JobStatusResponse(**job.to_dict())


@app.get("/jobs")
async def list_jobs(_: None = Depends(verify_token)):
    """List last 100 jobs."""
    return {"jobs": queue.list_jobs(100)}
