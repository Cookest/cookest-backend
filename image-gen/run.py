#!/usr/bin/env python3
"""Entry point — run as: python run.py"""
import uvicorn
from config import cfg

if __name__ == "__main__":
    uvicorn.run(
        "main:app",
        host=cfg.host,
        port=cfg.port,
        workers=1,           # single process; queue workers are async tasks
        log_level="info",
        access_log=True,
    )
