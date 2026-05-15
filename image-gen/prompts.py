"""Prompt engineering for food photography image generation.

Produces consistent, high-quality prompts tuned for:
  - Professional food photography aesthetic
  - Step-by-step cooking process images
  - Hero / plated dish images
"""
from __future__ import annotations

# ── Style prefix applied to every positive prompt ────────────────────────────

_FOOD_PHOTO_BASE = (
    "professional food photography, DSLR photo, sharp focus, "
    "natural window lighting, soft shadows, "
    "elegant minimalist composition, "
    "clean neutral background, "
    "8k resolution, ultra detailed, hyperrealistic"
)

_FOOD_PHOTO_NEGATIVE = (
    "text, watermark, logo, caption, banner, "
    "cartoon, anime, illustration, painting, drawing, sketch, "
    "ugly, blurry, out of focus, low resolution, low quality, "
    "extra hands, person, face, fingers, "
    "cluttered, messy background, oversaturated, harsh light, "
    "overexposed, underexposed, dark, grain, noise"
)

# ── Cuisine-to-background mapping (optional, for richer context) ─────────────

_CUISINE_PROPS: dict[str, str] = {
    "italian":      "rustic terracotta, fresh basil leaves, olive oil bottle",
    "french":       "linen tablecloth, small ceramic ramekin, fresh thyme",
    "portuguese":   "azulejo tile background, cork wood surface, sea salt",
    "spanish":      "terracotta tile, saffron threads, paprika",
    "japanese":     "dark slate board, bamboo chopsticks, wasabi",
    "chinese":      "red lacquer tray, ginger root, star anise",
    "indian":       "warm spice palette, brass bowl, curry leaves",
    "mexican":      "colorful ceramic plate, lime wedges, dried chilies",
    "american":     "cast-iron skillet, checkered cloth, sea salt flakes",
    "mediterranean": "olive wood board, lemon zest, sea salt",
    "nordic":       "grey stone slab, dill fronds, lingonberries",
    "greek":        "white marble surface, olives, feta crumble",
}


def step_prompt(
    step_description: str,
    recipe_name: str,
    step_index: int,
    total_steps: int,
    cuisine: str | None = None,
) -> tuple[str, str]:
    """Return (positive_prompt, negative_prompt) for a cooking step image."""
    cuisine_key = (cuisine or "").lower()
    props = _CUISINE_PROPS.get(cuisine_key, "wooden cutting board, fresh herbs")

    # Determine camera angle based on step type
    action = step_description.lower()
    if any(w in action for w in ["chop", "slice", "dice", "cut", "mince", "peel"]):
        angle = "top-down overhead shot"
        context = "knife work preparation"
    elif any(w in action for w in ["fry", "sauté", "sear", "boil", "simmer", "stir"]):
        angle = "45-degree angle shot"
        context = "active cooking in pan"
    elif any(w in action for w in ["bake", "roast", "grill", "oven"]):
        angle = "front-angle beauty shot"
        context = "oven or grill cooking"
    elif any(w in action for w in ["plate", "garnish", "serve", "drizzle", "decorate"]):
        angle = "top-down hero shot"
        context = "elegant plating and garnishing"
    elif any(w in action for w in ["mix", "whisk", "blend", "combine", "fold"]):
        angle = "close-up 45-degree"
        context = "mixing and combining ingredients"
    else:
        angle = "45-degree angle shot"
        context = "cooking preparation"

    # Step progression hint for visual continuity
    if step_index == 0:
        stage = "mise en place, raw ingredients laid out beautifully"
    elif step_index == total_steps - 1:
        stage = "finished dish, final plating, restaurant quality presentation"
    else:
        stage = f"step {step_index + 1} of cooking process, work in progress"

    positive = (
        f"{_FOOD_PHOTO_BASE}, "
        f"{recipe_name}, {context}, "
        f"{step_description}, "
        f"{angle}, "
        f"{stage}, "
        f"{props}"
    )
    return positive, _FOOD_PHOTO_NEGATIVE


def hero_prompt(
    recipe_name: str,
    description: str | None,
    cuisine: str | None = None,
    category: str | None = None,
) -> tuple[str, str]:
    """Return (positive_prompt, negative_prompt) for a hero / cover image."""
    cuisine_key = (cuisine or "").lower()
    props = _CUISINE_PROPS.get(cuisine_key, "elegant wooden board, fresh herbs")

    # Category-specific styling
    cat = (category or "").lower()
    if "dessert" in cat or "cake" in cat or "sweet" in cat:
        style = "pastel palette, fine pastry photography, sugar dusting"
    elif "soup" in cat or "stew" in cat:
        style = "steam rising, warm bowl, rustic tablecloth"
    elif "salad" in cat:
        style = "vibrant greens, fresh ingredients, top-down hero"
    elif "breakfast" in cat:
        style = "morning light, coffee cup beside, golden toast"
    else:
        style = "magazine-cover hero shot, perfect plating"

    positive = (
        f"{_FOOD_PHOTO_BASE}, "
        f"hero image of {recipe_name}, "
        f"{description or 'beautifully plated dish'}, "
        f"{style}, "
        f"top-down or 45-degree angle, "
        f"{props}, "
        f"Michelin-star presentation, award-winning food photography"
    )
    return positive, _FOOD_PHOTO_NEGATIVE
