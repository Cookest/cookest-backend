//! Time estimation and region classification algorithms for recipes

use regex::Regex;

/// Estimated cooking time split into components.
pub struct TimeEstimate {
    pub prep_time_min: i32,
    pub cook_time_min: i32,
    pub total_time_min: i32,
}

/// Estimate recipe prep/cook times deterministically.
///
/// Arguments:
///   step_instructions — slice of step instruction strings
///   num_ingredients   — number of ingredients
///   num_steps         — number of steps
///   category          — optional recipe category (e.g. "salad")
pub fn estimate_time(
    step_instructions: &[&str],
    num_ingredients: usize,
    num_steps: usize,
    category: Option<&str>,
) -> TimeEstimate {
    // Regex: capture numeric values before minute/min/hour/hr
    let re = Regex::new(r"(\d+)\s*(?:hour|hr)s?").unwrap();
    let re_min = Regex::new(r"(\d+)\s*(?:minute|min)s?").unwrap();

    let mut total_matched: i32 = 0;
    for instruction in step_instructions {
        for cap in re.captures_iter(instruction) {
            let hrs: i32 = cap[1].parse().unwrap_or(0);
            total_matched += hrs * 60;
        }
        for cap in re_min.captures_iter(instruction) {
            let mins: i32 = cap[1].parse().unwrap_or(0);
            total_matched += mins;
        }
    }

    let prep_time_min = 5 + (num_ingredients as f32 * 1.5) as i32;

    let (cook_time_min, total_time_min) = if total_matched > 0 {
        let cook = total_matched;
        (cook, prep_time_min + cook)
    } else {
        let is_salad = category
            .map(|c| c.to_lowercase() == "salad")
            .unwrap_or(false);
        let cook = if is_salad {
            0
        } else {
            10 + (num_steps as f32 * 2.0) as i32
        };
        let prep = prep_time_min + (num_steps as f32 * 1.0) as i32;
        let total = prep + cook;
        return TimeEstimate {
            prep_time_min: prep,
            cook_time_min: cook,
            total_time_min: total,
        };
    };

    TimeEstimate {
        prep_time_min,
        cook_time_min,
        total_time_min,
    }
}

/// Classify the cuisine region from ingredient names and tags.
///
/// Returns the region with the most keyword matches, or "International" if tied/none.
pub fn classify_region(ingredient_names: &[&str], tags: &[&str]) -> String {
    let cuisine_map: &[(&str, &[&str])] = &[
        (
            "Asian",
            &[
                "soy sauce",
                "ginger",
                "mirin",
                "sesame oil",
                "rice vinegar",
                "molho de soja",
                "gengibre",
            ],
        ),
        (
            "Italian",
            &[
                "olive oil",
                "oregano",
                "basil",
                "mozzarella",
                "parmesan",
                "pasta",
                "azeite",
                "manjericao",
            ],
        ),
        (
            "Mexican",
            &["cilantro", "tortilla", "jalapeno", "coentro", "taco"],
        ),
        (
            "Indian",
            &["curry", "garam masala", "turmeric", "cardamomo", "caril"],
        ),
        (
            "Portuguese",
            &["cod", "bacalhau", "chourico", "coentro", "batata"],
        ),
    ];

    let all_text: Vec<String> = ingredient_names
        .iter()
        .chain(tags.iter())
        .map(|s| s.to_lowercase())
        .collect();

    let mut best_region = "International".to_string();
    let mut best_count = 0usize;

    for (region, keywords) in cuisine_map {
        let count = keywords
            .iter()
            .filter(|&&kw| all_text.iter().any(|t| t.contains(kw)))
            .count();
        if count > best_count {
            best_count = count;
            best_region = region.to_string();
        }
    }

    best_region
}
