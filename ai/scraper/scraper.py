#!/usr/bin/env python3
"""Recipe Scraper and AI Normalizer Pipeline.

This script crawls recipe blogs/websites, extracts microdata using recipe-scrapers,
normalizes and cleans ingredients/macros/instructions using Ollama, and inserts 
the records directly into the Cookest food database.

Usage:
    # Set up environments
    export FOOD_DATABASE_URL=postgresql://postgres:postgres@localhost:5432/cookest_food
    export OLLAMA_URL=http://localhost:11434
    export OLLAMA_MODEL=llama3.1:8b

    # Scrape a single URL (dry run)
    python scraper.py --url https://www.allrecipes.com/recipe/15896/banana-banana-bread/ --dry-run

    # Scrape a sitemap (up to 10 recipes)
    python scraper.py --sitemap https://www.joyofbaking.com/sitemap.xml --pattern "recipes" --limit 10
"""

import argparse
import json
import os
import re
import sys
import time
import uuid
from pathlib import Path
from urllib.parse import urlparse

import requests
import psycopg
from bs4 import BeautifulSoup
from recipe_scrapers import scrape_me

# Default Fallbacks
DEFAULT_OLLAMA_URL = "http://localhost:11434"
DEFAULT_OLLAMA_MODEL = "llama3.1:8b"


def load_dotenv():
    """Locate and load .env file from parent directories into environment."""
    current = Path(__file__).resolve().parent
    for _ in range(5):
        env_path = current / ".env"
        if env_path.exists():
            with open(env_path, "r") as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        k, v = line.split("=", 1)
                        k = k.strip()
                        v = v.strip().strip("'").strip('"')
                        if k not in os.environ:
                            os.environ[k] = v
            break
        current = current.parent


# Load environment variables
load_dotenv()


def get_db_url(args_db_url=None) -> str:
    url = args_db_url or os.environ.get("FOOD_DATABASE_URL") or os.environ.get("DATABASE_URL")
    if not url:
        sys.exit("Error: Please set FOOD_DATABASE_URL environment variable or pass --db-url")
    return url


def get_ollama_url() -> str:
    return os.environ.get("OLLAMA_URL", DEFAULT_OLLAMA_URL).rstrip("/")


def get_ollama_model(args_model=None) -> str:
    return args_model or os.environ.get("OLLAMA_MODEL") or DEFAULT_OLLAMA_MODEL


def slugify(text: str) -> str:
    """Create a URL-safe slug."""
    text = text.lower()
    # Replace non-alphanumeric with hyphens
    text = re.sub(r"[^a-z0-9]+", "-", text)
    # Remove leading/trailing hyphens
    return text.strip("-")


def discover_urls_from_sitemap(sitemap_url: str, pattern: str = None) -> list[str]:
    """Recursively fetch and parse sitemap or sitemap index to find matching URLs."""
    print(f"Fetching sitemap: {sitemap_url}")
    headers = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"}
    try:
        resp = requests.get(sitemap_url, timeout=30, headers=headers)
        resp.raise_for_status()
    except Exception as e:
        print(f"Error fetching sitemap {sitemap_url}: {e}")
        return []

    soup = BeautifulSoup(resp.content, "xml")
    urls = []

    # Check if sitemap index
    sitemaps = [loc.text.strip() for loc in soup.find_all("sitemap")]
    if sitemaps:
        print(f"Found sitemap index with {len(sitemaps)} sub-sitemaps. Parsing...")
        for sub in sitemaps:
            urls.extend(discover_urls_from_sitemap(sub, pattern))
        return urls

    # Parse regular sitemap URLs
    for url_tag in soup.find_all("url"):
        loc = url_tag.find("loc")
        if loc:
            url = loc.text.strip()
            if pattern:
                if re.search(pattern, url, re.IGNORECASE):
                    urls.append(url)
            else:
                urls.append(url)
    
    return list(set(urls))


def extract_raw_recipe(url: str) -> dict:
    """Use recipe-scrapers to extract raw structured microdata."""
    print(f"Scraping raw web microdata from: {url}")
    scraper = scrape_me(url)
    
    # Extract instructions list
    instructions = []
    try:
        instructions = scraper.instructions_list()
    except Exception:
        pass
    if not instructions:
        try:
            raw_inst = scraper.instructions()
            if raw_inst:
                instructions = [s.strip() for s in raw_inst.split("\n") if s.strip()]
        except Exception:
            pass

    raw_data = {
        "title": scraper.title(),
        "total_time": scraper.total_time(),
        "yields": scraper.yields(),
        "ingredients": scraper.ingredients(),
        "instructions": instructions,
        "image": scraper.image(),
        "nutrients": scraper.nutrients(),
        "cuisine": scraper.cuisine(),
        "category": scraper.category(),
        "host": scraper.host(),
    }
    return raw_data


def normalize_with_ollama(raw_data: dict, model: str) -> dict:
    """Send raw recipe details to Ollama to verify, normalize and structure."""
    print(f"Calling Ollama ({model}) to verify and normalize recipe...")
    
    system_prompt = (
        "You are an expert dietitian, translation assistant, and structured culinary database parser.\n"
        "Your task is to take raw recipe details and normalize them into a clean JSON object.\n"
        "CRITICAL: Translate all textual fields (cuisine, category, ingredient name, ingredient unit, ingredient notes, steps) into English. The database strictly expects English records.\n"
        "Rules:\n"
        "1. Ingredients list:\n"
        "   - 'name': Singular, lowercase, canonical English ingredient name (e.g. 'spinach', NOT 'shredded spinach' and NOT Spanish 'espinacas').\n"
        "   - 'quantity': Decimal value or null if unknown.\n"
        "   - 'unit': Singular unit in English (e.g. 'cup', 'tbsp', 'g', 'ml', 'piece', 'can') or null.\n"
        "   - 'notes': Preparation description in English (e.g. 'shredded', 'melted', 'divided').\n"
        "   - 'category': Categorize into exactly one of: 'protein', 'dairy', 'vegetable', 'grain', 'fruit', 'fat', 'spice', 'sweetener', 'other'.\n"
        "   - 'quantity_grams': Estimate weight in grams of this quantity. Crucial for macro calculations. Be realistic (e.g. 1 egg is 50g, 1 cup flour is 120g, 1 tbsp olive oil is 14g).\n"
        "2. Instructions list:\n"
        "   - Clean list of sequential numbered steps, fully translated into English. Strip website boilerplate, advertisements, or stories.\n"
        "3. Macros (per serving):\n"
        "   - Estimate macros if missing or unrealistic. Return 'calories', 'protein_g', 'carbs_g', 'fat_g', 'fiber_g', 'sugar_g', 'sodium_mg', 'saturated_fat_g'.\n"
        "4. Determine flags: 'is_vegetarian', 'is_vegan', 'is_gluten_free', 'is_dairy_free', 'is_nut_free' based on the ingredients list."
    )

    prompt = f"Raw Recipe Data:\n{json.dumps(raw_data, indent=2)}\n\nOutput only a valid JSON object matching this schema:\n" + """{
  "cuisine": string or null,
  "category": "breakfast" | "lunch" | "dinner" | "snack" | "dessert" | null,
  "difficulty": "easy" | "medium" | "hard",
  "servings": integer (fallback to 2 if missing),
  "prep_time_min": integer or null,
  "cook_time_min": integer or null,
  "total_time_min": integer or null,
  "is_vegetarian": boolean,
  "is_vegan": boolean,
  "is_gluten_free": boolean,
  "is_dairy_free": boolean,
  "is_nut_free": boolean,
  "ingredients": [
    {
      "name": string,
      "quantity": number or null,
      "unit": string or null,
      "quantity_grams": number,
      "notes": string or null,
      "category": "protein" | "dairy" | "vegetable" | "grain" | "fruit" | "fat" | "spice" | "sweetener" | "other"
    }
  ],
  "steps": [string],
  "nutrition": {
    "calories": number,
    "protein_g": number,
    "carbs_g": number,
    "fat_g": number,
    "fiber_g": number,
    "sugar_g": number,
    "sodium_mg": number,
    "saturated_fat_g": number
  }
}"""

    url = f"{get_ollama_url()}/api/generate"
    payload = {
        "model": model,
        "prompt": prompt,
        "system": system_prompt,
        "stream": False,
        "format": "json",
        "options": {
            "temperature": 0.1
        }
    }

    resp = requests.post(url, json=payload, timeout=120)
    resp.raise_for_status()
    
    response_body = resp.json()
    raw_response = response_body.get("response", "").strip()
    
    normalized = json.loads(raw_response)
    return normalized


def insert_into_database(conn: psycopg.Connection, url: str, raw_data: dict, norm_data: dict):
    """Insert normalized recipe details into PostgreSQL in a single transaction."""
    parsed_url = urlparse(url)
    site_domain = parsed_url.netloc.replace("www.", "")

    title = raw_data["title"]
    slug = f"{slugify(title)}-{uuid.uuid4().hex[:8]}"

    with conn.cursor() as cur:
        # 1. Insert into recipes table
        cur.execute(
            """
            INSERT INTO recipes (
                name, slug, description, cuisine, category, difficulty, servings, 
                prep_time_min, cook_time_min, total_time_min, 
                is_vegetarian, is_vegan, is_gluten_free, is_dairy_free, is_nut_free, 
                source_url, created_at, updated_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, NOW(), NOW())
            RETURNING id
            """,
            (
                title,
                slug,
                raw_data.get("description") or title,
                norm_data.get("cuisine"),
                norm_data.get("category"),
                norm_data.get("difficulty"),
                norm_data.get("servings", 2),
                norm_data.get("prep_time_min") or raw_data.get("total_time", 0) // 3,
                norm_data.get("cook_time_min") or raw_data.get("total_time", 0) // 3 * 2,
                norm_data.get("total_time_min") or raw_data.get("total_time"),
                norm_data.get("is_vegetarian", False),
                norm_data.get("is_vegan", False),
                norm_data.get("is_gluten_free", False),
                norm_data.get("is_dairy_free", False),
                norm_data.get("is_nut_free", False),
                url,
            )
        )
        recipe_id = cur.fetchone()[0]

        # 2. Ingest and link ingredients
        for idx, ing in enumerate(norm_data.get("ingredients", [])):
            name = ing["name"].strip().lower()
            if not name:
                continue
            category = ing.get("category", "other")
            
            # Upsert ingredient catalog
            cur.execute(
                """
                INSERT INTO ingredients (name, category, created_at)
                VALUES (%s, %s, NOW())
                ON CONFLICT (name) DO UPDATE SET category = COALESCE(ingredients.category, EXCLUDED.category)
                RETURNING id
                """,
                (name, category)
            )
            ing_id = cur.fetchone()[0]

            # Link recipe to ingredient
            cur.execute(
                """
                INSERT INTO recipe_ingredients (
                    recipe_id, ingredient_id, quantity, unit, quantity_grams, notes, display_order
                ) VALUES (%s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    recipe_id,
                    ing_id,
                    ing.get("quantity"),
                    ing.get("unit"),
                    ing.get("quantity_grams"),
                    ing.get("notes"),
                    idx
                )
            )

        # 3. Insert instructions (recipe_steps)
        steps = norm_data.get("steps", [])
        if not steps:
            steps = raw_data.get("instructions", [])
        
        for step_num, inst in enumerate(steps, start=1):
            cur.execute(
                """
                INSERT INTO recipe_steps (recipe_id, step_number, instruction)
                VALUES (%s, %s, %s)
                """,
                (recipe_id, step_num, inst.strip())
            )

        # 4. Insert image URL if present
        if raw_data.get("image"):
            cur.execute(
                """
                INSERT INTO recipe_images (recipe_id, url, image_type, is_primary, created_at)
                VALUES (%s, %s, 'hero', true, NOW())
                """,
                (recipe_id, raw_data["image"])
            )

        # 5. Insert nutrition
        nut = norm_data.get("nutrition")
        if nut:
            cur.execute(
                """
                INSERT INTO recipe_nutrition (
                    recipe_id, per_serving, calories, protein_g, carbs_g, fat_g, fiber_g, sugar_g, sodium_mg, saturated_fat_g, calculated_at
                ) VALUES (%s, true, %s, %s, %s, %s, %s, %s, %s, %s, NOW())
                """,
                (
                    recipe_id,
                    nut.get("calories"),
                    nut.get("protein_g"),
                    nut.get("carbs_g"),
                    nut.get("fat_g"),
                    nut.get("fiber_g"),
                    nut.get("sugar_g"),
                    nut.get("sodium_mg"),
                    nut.get("saturated_fat_g")
                )
            )

        # 6. Log success to etl_scrape_log
        cur.execute(
            """
            INSERT INTO etl_scrape_log (source_url, source_site, scraped_at, status, recipe_id, error_msg)
            VALUES (%s, %s, NOW(), 'success', %s, NULL)
            ON CONFLICT (source_url) DO UPDATE SET 
                scraped_at = EXCLUDED.scraped_at,
                status = EXCLUDED.status,
                recipe_id = EXCLUDED.recipe_id,
                error_msg = EXCLUDED.error_msg
            """,
            (url, site_domain, recipe_id)
        )

    print(f"Successfully saved recipe: {title} (ID: {recipe_id})")
    return recipe_id


def log_error_in_db(conn: psycopg.Connection, url: str, error_msg: str):
    """Log an execution failure to etl_scrape_log table so we track it."""
    parsed_url = urlparse(url)
    site_domain = parsed_url.netloc.replace("www.", "")
    try:
        with conn.cursor() as cur:
            cur.execute(
                """
                INSERT INTO etl_scrape_log (source_url, source_site, scraped_at, status, recipe_id, error_msg)
                VALUES (%s, %s, NOW(), 'error', NULL, %s)
                ON CONFLICT (source_url) DO UPDATE SET 
                    scraped_at = EXCLUDED.scraped_at,
                    status = EXCLUDED.status,
                    recipe_id = EXCLUDED.recipe_id,
                    error_msg = EXCLUDED.error_msg
                """,
                (url, site_domain, error_msg[:1000])
            )
            conn.commit()
    except Exception as e:
        print(f"Warning: Could not log error to database: {e}")


def is_already_scraped(conn: psycopg.Connection, url: str) -> bool:
    """Check if the URL is already successfully scraped and stored."""
    try:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT status FROM etl_scrape_log WHERE source_url = %s",
                (url,)
            )
            row = cur.fetchone()
            return row is not None and row[0] == 'success'
    except Exception:
        return False


def run_pipeline(url: str, db_conn: psycopg.Connection | None, model: str, dry_run: bool):
    """Coordinate the scraping, LLM normalization, and DB insertion for a single URL."""
    try:
        # Step 1: Scrape
        raw = extract_raw_recipe(url)
        if not raw.get("title") or not raw.get("ingredients"):
            raise ValueError("Recipe scraping resulted in empty title or ingredients.")

        # Step 2: Ollama Normalization
        norm = normalize_with_ollama(raw, model)

        if dry_run:
            print("\n=== DRY RUN RESULTS (NO DATABASE WRITE) ===")
            print(f"Title: {raw['title']}")
            print(f"Image: {raw.get('image')}")
            print(f"Cuisine: {norm.get('cuisine')}, Category: {norm.get('category')}, Servings: {norm.get('servings')}")
            print(f"Difficulty: {norm.get('difficulty')}, Times: Prep={norm.get('prep_time_min')}m, Cook={norm.get('cook_time_min')}m")
            print("Ingredients:")
            for ing in norm.get("ingredients", []):
                print(f"  - {ing.get('quantity')} {ing.get('unit')} {ing.get('name')} ({ing.get('quantity_grams')}g) [notes: {ing.get('notes')}, cat: {ing.get('category')}]")
            print("Steps:")
            for step in norm.get("steps", []):
                print(f"  {step}")
            print(f"Macros: {json.dumps(norm.get('nutrition'), indent=2)}")
            print("===========================================\n")
            return True

        # Step 3: DB Insert
        if db_conn:
            db_conn.commit()  # Flush any previous states
            try:
                insert_into_database(db_conn, url, raw, norm)
                db_conn.commit()
                return True
            except Exception as e:
                db_conn.rollback()
                raise e
    except Exception as e:
        print(f"[-] Pipeline error on URL {url}: {e}")
        if db_conn and not dry_run:
            log_error_in_db(db_conn, url, str(e))
        return False


def main():
    parser = argparse.ArgumentParser(description="Cookest Recipe Crawler and AI Normalizer Pipeline")
    parser.add_argument("--url", help="A single recipe URL to scrape")
    parser.add_argument("--sitemap", help="Sitemap URL containing recipe links")
    parser.add_argument("--pattern", help="Regex pattern to filter URLs discovered from sitemap")
    parser.add_argument("--config", help="Path to a JSON configuration file containing a list of sitemaps to crawl")
    parser.add_argument("--limit", type=int, default=10, help="Max recipes to scrape in this run (default: 10)")
    parser.add_argument("--model", help=f"Ollama model (default: read OLLAMA_MODEL or '{DEFAULT_OLLAMA_MODEL}')")
    parser.add_argument("--db-url", help="PostgreSQL connection string")
    parser.add_argument("--dry-run", action="store_true", help="Do not write to the database")
    parser.add_argument("--force", action="store_true", help="Force scraping even if already successfully scraped")
    parser.add_argument("--delay", type=float, default=2.0, help="Delay in seconds between requests (default: 2.0)")
    args = parser.parse_args()

    if not args.url and not args.sitemap and not args.config:
        parser.print_help()
        sys.exit("\nError: Please provide either --url, --sitemap, or --config")

    model = get_ollama_model(args.model)
    print(f"AI Model: {model}")
    print(f"Ollama endpoint: {get_ollama_url()}")

    db_conn = None
    if not args.dry_run:
        db_url = get_db_url(args.db_url)
        print("Connecting to database...")
        db_conn = psycopg.connect(db_url)
        print("Connected successfully.")

    # 1. Process single URL
    if args.url:
        if db_conn and not args.force and is_already_scraped(db_conn, args.url):
            print(f"Skipping already scraped URL: {args.url} (Use --force to override)")
            return
        
        success = run_pipeline(args.url, db_conn, model, args.dry_run)
        if not success:
            sys.exit(1)

    # 2. Process multi-site config file
    elif args.config:
        config_path = Path(args.config)
        if not config_path.exists():
            sys.exit(f"Error: Config file not found at {args.config}")
        with open(config_path, "r", encoding="utf-8") as f:
            try:
                targets = json.load(f)
            except Exception as e:
                sys.exit(f"Error parsing JSON config: {e}")
        
        if not isinstance(targets, list):
            sys.exit("Error: Config JSON must be a list of objects.")
        
        print(f"Loaded config with {len(targets)} sitemap targets.")
        
        for t_idx, target in enumerate(targets, start=1):
            sitemap_url = target.get("sitemap")
            if not sitemap_url:
                print(f"[-] Skipping target #{t_idx} (missing 'sitemap' field)")
                continue
            
            pattern = target.get("pattern")
            limit = target.get("limit", args.limit)
            
            print(f"\n==========================================")
            print(f"Target [{t_idx}/{len(targets)}]: {sitemap_url}")
            print(f"Pattern: {pattern}, Limit: {limit}")
            print(f"==========================================")
            
            all_urls = discover_urls_from_sitemap(sitemap_url, pattern)
            print(f"Discovered {len(all_urls)} URLs.")
            
            queue = []
            for url in all_urls:
                if db_conn and not args.force and is_already_scraped(db_conn, url):
                    continue
                queue.append(url)
            
            print(f"Queue has {len(queue)} pending URLs (after deduplication). Limit set to {limit}.")
            queue = queue[:limit]
            
            success_count = 0
            for idx, url in enumerate(queue, start=1):
                print(f"\n[{idx}/{len(queue)}] Processing: {url}")
                success = run_pipeline(url, db_conn, model, args.dry_run)
                if success:
                    success_count += 1
                if idx < len(queue):
                    time.sleep(args.delay)
            print(f"Finished target. Successfully processed {success_count}/{len(queue)} recipes.")

    # 3. Process single sitemap
    elif args.sitemap:
        all_urls = discover_urls_from_sitemap(args.sitemap, args.pattern)
        print(f"Discovered {len(all_urls)} URLs from sitemap.")
        
        # Filter urls already scraped to avoid double-processing
        queue = []
        for url in all_urls:
            if db_conn and not args.force and is_already_scraped(db_conn, url):
                continue
            queue.append(url)
        
        print(f"Queue has {len(queue)} pending URLs (after deduplication). Limit set to {args.limit}.")
        queue = queue[:args.limit]

        success_count = 0
        for idx, url in enumerate(queue, start=1):
            print(f"\n[{idx}/{len(queue)}] Processing: {url}")
            success = run_pipeline(url, db_conn, model, args.dry_run)
            if success:
                success_count += 1
            if idx < len(queue):
                time.sleep(args.delay)

        print(f"\nDone! Successfully processed {success_count}/{len(queue)} recipes.")

    if db_conn:
        db_conn.close()


if __name__ == "__main__":
    main()
