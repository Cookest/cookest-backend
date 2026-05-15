"""Async job queue for image generation requests.

Uses an asyncio Queue + background worker task to process jobs
one-at-a-time (or concurrently if num_workers > 1 in config).
"""
from __future__ import annotations

import asyncio
import logging
import time
import uuid
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from config import cfg
from generator import generate

logger = logging.getLogger("image_gen.queue")


class JobStatus(str, Enum):
    PENDING = "pending"
    GENERATING = "generating"
    DONE = "done"
    FAILED = "failed"


@dataclass
class Job:
    id: str = field(default_factory=lambda: str(uuid.uuid4()))
    positive_prompt: str = ""
    negative_prompt: str = ""
    seed: int | None = None
    # Caller-supplied metadata (recipe_id, step_index, etc.) — passed back in result
    metadata: dict[str, Any] = field(default_factory=dict)

    status: JobStatus = JobStatus.PENDING
    image_url: str | None = None
    error: str | None = None
    created_at: float = field(default_factory=time.time)
    finished_at: float | None = None

    def to_dict(self) -> dict:
        return {
            "job_id": self.id,
            "status": self.status.value,
            "image_url": self.image_url,
            "error": self.error,
            "metadata": self.metadata,
            "created_at": self.created_at,
            "finished_at": self.finished_at,
        }


class JobQueue:
    def __init__(self):
        self._queue: asyncio.Queue[Job] = asyncio.Queue(maxsize=cfg.max_queue_size)
        self._jobs: dict[str, Job] = {}  # job_id → Job
        self._workers: list[asyncio.Task] = []

    async def start(self) -> None:
        """Start background worker tasks."""
        for i in range(cfg.num_workers):
            task = asyncio.create_task(self._worker(i), name=f"img-worker-{i}")
            self._workers.append(task)
        logger.info("Started %d image generation worker(s)", cfg.num_workers)

    async def stop(self) -> None:
        for w in self._workers:
            w.cancel()
        await asyncio.gather(*self._workers, return_exceptions=True)
        logger.info("Image generation workers stopped")

    async def submit(
        self,
        positive_prompt: str,
        negative_prompt: str,
        seed: int | None = None,
        metadata: dict | None = None,
    ) -> Job:
        """Enqueue a generation job. Raises if queue is full."""
        job = Job(
            positive_prompt=positive_prompt,
            negative_prompt=negative_prompt,
            seed=seed,
            metadata=metadata or {},
        )
        self._jobs[job.id] = job
        try:
            self._queue.put_nowait(job)
        except asyncio.QueueFull:
            job.status = JobStatus.FAILED
            job.error = "Queue is full, try again later"
        return job

    def get(self, job_id: str) -> Job | None:
        return self._jobs.get(job_id)

    def list_jobs(self, limit: int = 100) -> list[dict]:
        return [j.to_dict() for j in list(self._jobs.values())[-limit:]]

    async def _worker(self, worker_id: int) -> None:
        logger.info("Worker %d started", worker_id)
        while True:
            try:
                job: Job = await self._queue.get()
                job.status = JobStatus.GENERATING
                logger.info("[worker %d] Generating job %s", worker_id, job.id)

                try:
                    url = await generate(
                        job.positive_prompt,
                        job.negative_prompt,
                        job.seed,
                    )
                    job.image_url = url
                    job.status = JobStatus.DONE
                except Exception as exc:
                    logger.exception("[worker %d] Job %s failed", worker_id, job.id)
                    job.status = JobStatus.FAILED
                    job.error = str(exc)

                job.finished_at = time.time()
                self._queue.task_done()

            except asyncio.CancelledError:
                break
            except Exception:
                logger.exception("Unexpected error in worker %d", worker_id)


# Singleton instance
queue = JobQueue()
