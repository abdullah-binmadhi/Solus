//! Tailored Recommendation Engine & Astrological Persona Matrix.
//! Evaluates user taste questions against curated anime metadata with zero AI slop.

use crate::db::cache::Anime;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A single option for a questionnaire question.
#[derive(Debug, Clone)]
pub struct TailoredOption {
    pub label: &'static str,
    pub detail: &'static str,
    pub symbol: &'static str,
    pub tags_pos: &'static [(&'static str, f32)],
    pub tags_neg: &'static [&'static str],
    pub min_episodes: Option<i64>,
    pub max_episodes: Option<i64>,
    pub min_year: Option<i64>,
    pub max_year: Option<i64>,
    pub status: Option<&'static str>,
}

/// A questionnaire question.
#[derive(Debug, Clone)]
pub struct TailoredQuestion {
    #[allow(dead_code)]
    pub id: usize,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub symbol: &'static str,
    pub is_multi_select: bool,
    pub options: Vec<TailoredOption>,
}

/// User choices for all 10 questions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct TailoredAnswers {
    pub choices: [usize; 9],
    pub deal_breakers: HashSet<usize>,
}

/// User choices for the 6 Tailored Refinement Crucible questions.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TailoredRefinement {
    pub trope_selections: HashSet<usize>,
    pub pacing_selections: HashSet<usize>,
    pub visual_selections: HashSet<usize>,
    pub aftertaste_selections: HashSet<usize>,
    pub rarity_selections: HashSet<usize>,
    pub anchor_anime_id: Option<i64>,
    pub anchor_title: Option<String>,
}


/// Dynamic Anime Soul Persona calculated from user answers.
#[derive(Debug, Clone)]
pub struct Persona {
    pub title: &'static str,
    pub epithet: &'static str,
    #[allow(dead_code)]
    pub symbol: &'static str,
    pub primary_color: Color,
    pub accent_color: Color,
    pub gauges: [(&'static str, u8); 5],
    pub resonance_tags: Vec<&'static str>,
}

/// A scored anime recommendation with match percentage and reason badges.
#[derive(Debug, Clone)]
pub struct TailoredMatch {
    pub anime: Anime,
    pub match_percentage: u8,
    pub reason_tags: Vec<String>,
}

/// Real-time interactive tuner controls applied instantaneously over the 120-item scored pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTunerConfig {
    /// 0 = All Picks, 1 = Hidden Gems, 2 = Acclaimed (82%+), 3 = Modern (2022+)
    pub discovery_tier: usize,
    /// Hide Season 2+ / Part 2+ sequels unless toggled off
    pub hide_sequels: bool,
    /// Only show FINISHED series
    pub finished_only: bool,
    /// Tags boosted with `+` in the Live Vibe Tuner
    pub boosted_tags: HashSet<String>,
    /// Tags excluded with `-` in the Live Vibe Tuner
    pub muted_tags: HashSet<String>,
    /// Anime IDs dismissed directly with `[x]`
    pub dismissed_ids: HashSet<i64>,
    /// Locked "More Like This" `[m]` show: (anime_id, title, genres)
    pub anchor_anime: Option<(i64, String, Vec<String>)>,
    /// AniList community peer IDs for `anchor_anime`
    pub anchor_peer_ids: HashSet<i64>,
}

impl Default for LiveTunerConfig {
    fn default() -> Self {
        Self {
            discovery_tier: 0,
            hide_sequels: true,
            finished_only: false,
            boosted_tags: HashSet::new(),
            muted_tags: HashSet::new(),
            dismissed_ids: HashSet::new(),
            anchor_anime: None,
            anchor_peer_ids: HashSet::new(),
        }
    }
}

impl LiveTunerConfig {
    pub fn has_active_modifiers(&self) -> bool {
        self.discovery_tier != 0
            || !self.hide_sequels
            || self.finished_only
            || !self.boosted_tags.is_empty()
            || !self.muted_tags.is_empty()
            || !self.dismissed_ids.is_empty()
            || self.anchor_anime.is_some()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Return the master 10 questions with custom Final Fantasy heraldry glyphs.
pub fn get_questions() -> Vec<TailoredQuestion> {
    vec![
        TailoredQuestion {
            id: 1,
            title: "Current Mood & Emotional Resonance",
            subtitle: "What energy do you want to feel while watching?",
            symbol: "󰋽",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "High Adrenaline & Hype",
                    detail: "Spectacle action, pulse-pounding choreography, triumphal energy",
                    symbol: "",
                    tags_pos: &[("Action", 35.0), ("Super Power", 25.0), ("Martial Arts", 20.0)],
                    tags_neg: &["Slice of Life"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Deep & Mind-Bending",
                    detail: "Psychological intrigue, complex conspiracies, unpredictable twists",
                    symbol: "◈",
                    tags_pos: &[("Psychological", 40.0), ("Mystery", 30.0), ("Thriller", 25.0)],
                    tags_neg: &["Ecchi"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Cozy & Heartwarming",
                    detail: "Comforting atmosphere, slow peaceful moments, low tension",
                    symbol: "◈",
                    tags_pos: &[("Slice of Life", 40.0), ("Comedy", 25.0), ("Iyashikei", 30.0)],
                    tags_neg: &["Horror", "Gore", "Tragedy"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Emotional Rollercoaster",
                    detail: "Intense romance, bittersweet farewells, heartfelt tear-jerker",
                    symbol: "",
                    tags_pos: &[("Drama", 35.0), ("Romance", 30.0), ("Tragedy", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Dark, Bleak & Gritty",
                    detail: "High mortality, existential stakes, unforgiving moral grayness",
                    symbol: "",
                    tags_pos: &[("Horror", 35.0), ("Dark Fantasy", 30.0), ("Survival", 25.0)],
                    tags_neg: &["Slice of Life"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 2,
            title: "Pacing & Time Commitment",
            subtitle: "How many episodes are you looking to commit?",
            symbol: "◈",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "Single Sitting (Movie / OVA)",
                    detail: "Complete self-contained story in 1 to 4 episodes",
                    symbol: "",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: Some(1), max_episodes: Some(4), min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Weekend Binge (10–13 Episodes)",
                    detail: "Fast, punchy single cour — tight pacing with no filler",
                    symbol: "◈",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: Some(10), max_episodes: Some(14), min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Full Season Arc (22–26 Episodes)",
                    detail: "Double cour narrative with rich buildup and satisfying closure",
                    symbol: "󰋽",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: Some(20), max_episodes: Some(28), min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Grand Epic Saga (40+ Episodes)",
                    detail: "Deep world-building, sprawling character evolutions across arcs",
                    symbol: "",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: Some(38), max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Any Length (Best Story Wins)",
                    detail: "No constraint — let quality dictate runtime",
                    symbol: "◈",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 3,
            title: "Realm Setting & Atmosphere",
            subtitle: "What world backdrop speaks to you?",
            symbol: "",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "High Fantasy & Ancient Magic",
                    detail: "Grand kingdoms, arcane spells, guild adventures, legendary beasts",
                    symbol: "󰋽",
                    tags_pos: &[("Fantasy", 40.0), ("Magic", 25.0), ("Adventure", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Cyberpunk & Synthetic Future",
                    detail: "Neon megacities, cybernetics, AI consciousness, high-tech dystopia",
                    symbol: "",
                    tags_pos: &[("Sci-Fi", 40.0), ("Cyberpunk", 35.0), ("Dystopian", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Modern City & Contemporary Life",
                    detail: "Urban streets, supernatural underground, relatable realism",
                    symbol: "◈",
                    tags_pos: &[("Urban Fantasy", 30.0), ("Mystery", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Historical & Feudal Era",
                    detail: "Samurai duels, historical dynasties, ancient battles, honor",
                    symbol: "◈",
                    tags_pos: &[("Historical", 40.0), ("Swordplay", 25.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Isekai & Otherworldly Rebirth",
                    detail: "Transported or awakened in a foreign realm with unique laws",
                    symbol: "",
                    tags_pos: &[("Isekai", 40.0), ("Fantasy", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 4,
            title: "Protagonist Archetype",
            subtitle: "What style of hero do you want to follow?",
            symbol: "",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "The Calculated Mastermind",
                    detail: "Genius tactician who outthinks enemies 5 steps ahead",
                    symbol: "󰋽",
                    tags_pos: &[("Smart Male Lead", 35.0), ("Anti-Hero", 20.0), ("Psychological", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "The Scrappy Underdog",
                    detail: "Starts from the bottom, overcomes defeat through sheer willpower",
                    symbol: "",
                    tags_pos: &[("Underdog", 30.0), ("Coming of Age", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "The Calm & Overpowered Veteran",
                    detail: "Experienced, composed, casually dominant when pushed",
                    symbol: "◈",
                    tags_pos: &[("Overpowered Main Character", 30.0), ("Primarily Adult Cast", 15.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "The Relatable Everyday Survivor",
                    detail: "Normal person grounded in realism navigating chaotic forces",
                    symbol: "◈",
                    tags_pos: &[("Coming of Age", 25.0), ("Drama", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "The Dynamic Ensemble Fellowship",
                    detail: "No single hero — balanced cast with interlocking storylines",
                    symbol: "",
                    tags_pos: &[("Ensemble Cast", 35.0), ("Adventure", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 5,
            title: "Core Conflict Dynamics",
            subtitle: "What drives the tension forward?",
            symbol: "",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "Spectacular Martial & Spell Combat",
                    detail: "High-level choreography, flashy powers, thrilling battle climaxes",
                    symbol: "",
                    tags_pos: &[("Action", 35.0), ("Martial Arts", 30.0), ("Super Power", 25.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Intellectual Cat-and-Mouse Games",
                    detail: "Information warfare, detective deduction, high-stakes psychology",
                    symbol: "◈",
                    tags_pos: &[("Detective", 35.0), ("Mystery", 30.0), ("Psychological", 25.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Intimate Romantic & Personal Tension",
                    detail: "Sparks, chemistry, personal obstacles, emotional honesty",
                    symbol: "◈",
                    tags_pos: &[("Romance", 40.0), ("Drama", 25.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Surviving a Hostile Environment",
                    detail: "Death games, harsh wastelands, constant threat of death",
                    symbol: "",
                    tags_pos: &[("Survival", 35.0), ("Death Game", 30.0), ("Horror", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Comedic Misadventures & Chaos",
                    detail: "Bickering parties, chaotic misunderstandings, genuine laughs",
                    symbol: "󰋽",
                    tags_pos: &[("Comedy", 40.0), ("Parody", 25.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 6,
            title: "Darkness & Stakes Level",
            subtitle: "How grim and serious can the world get?",
            symbol: "",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "Uplifting & Inspiring",
                    detail: "Comforting tone where good triumphs and hope endures",
                    symbol: "◈",
                    tags_pos: &[("Wholesome", 30.0), ("Adventure", 20.0)],
                    tags_neg: &["Gore", "Tragedy", "Horror"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Balanced (Serious with Light)",
                    detail: "Consequential stakes, character growth, but retains hope",
                    symbol: "",
                    tags_pos: &[("Adventure", 20.0), ("Drama", 15.0)],
                    tags_neg: &["Gore"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Morally Gray & Complex",
                    detail: "Nuanced ethics, difficult sacrifices, no black-and-white morality",
                    symbol: "◈",
                    tags_pos: &[("Seinen", 30.0), ("Psychological", 25.0), ("Philosophy", 20.0)],
                    tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Brutal, Unforgiving & Visceral",
                    detail: "High body count, psychological trauma, graphic danger",
                    symbol: "󰋽",
                    tags_pos: &[("Horror", 35.0), ("Gore", 30.0), ("Tragedy", 25.0)],
                    tags_neg: &["Slice of Life"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 7,
            title: "Visual Era & Animation Craft",
            subtitle: "What visual era matches your eyes?",
            symbol: "◈",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "Cutting-Edge Modern (2020s)",
                    detail: "Ultra-crisp digital cinematography, fluid composite effects",
                    symbol: "",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: Some(2020), max_year: None, status: None,
                },
                TailoredOption {
                    label: "Modern Golden Classics (2010–2019)",
                    detail: "Peak studio adaptations, legendary art style consistency",
                    symbol: "󰋽",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: Some(2010), max_year: Some(2019), status: None,
                },
                TailoredOption {
                    label: "Retro Hand-Drawn Cels (90s – 2000s)",
                    detail: "Organic hand-painted grain, vintage aesthetic, classic feel",
                    symbol: "◈",
                    tags_pos: &[("Classic", 30.0)], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: Some(2009), status: None,
                },
                TailoredOption {
                    label: "Timeless (Any Release Year)",
                    detail: "Story and direction triumph over release era",
                    symbol: "",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 8,
            title: "Completion Status",
            subtitle: "Do you need a full conclusion right now?",
            symbol: "󰋽",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "100% Concluded Story",
                    detail: "Finished finale — no cliffhangers or waiting years for sequels",
                    symbol: "◈",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: Some("FINISHED"),
                },
                TailoredOption {
                    label: "Active & Airing in Broadcast",
                    detail: "Fresh currently broadcasting anime with active community talk",
                    symbol: "",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: Some("RELEASING"),
                },
                TailoredOption {
                    label: "Ongoing Multi-Season Journey",
                    detail: "Fine with unfinished adaptations if multiple seasons exist",
                    symbol: "◈",
                    tags_pos: &[], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 9,
            title: "Narrative Depth & Lore",
            subtitle: "What level of world complexity do you want?",
            symbol: "◈",
            is_multi_select: false,
            options: vec![
                TailoredOption {
                    label: "Direct & Entertaining (Popcorn Fun)",
                    detail: "Instant gratification, straightforward goals, zero headache",
                    symbol: "",
                    tags_pos: &[("Comedy", 20.0), ("Adventure", 15.0)], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Thought-Provoking & Philosophical",
                    detail: "Questions nature of humanity, ethics, existential puzzles",
                    symbol: "󰋽",
                    tags_pos: &[("Philosophy", 35.0), ("Psychological", 30.0)], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Sprawling Politics & World Lore",
                    detail: "Deep faction politics, strategic geography, ancient history",
                    symbol: "",
                    tags_pos: &[("Politics", 35.0), ("War", 25.0), ("Fantasy", 20.0)], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Intimate Character Psychology",
                    detail: "Nuanced emotional flaws, deep character relationships",
                    symbol: "",
                    tags_pos: &[("Drama", 35.0), ("Coming of Age", 25.0)], tags_neg: &[],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 10,
            title: "Absolute Deal-Breakers",
            subtitle: "Toggle tags you strictly want excluded (Multi-Select)",
            symbol: "",
            is_multi_select: true,
            options: vec![
                TailoredOption {
                    label: "No Excessive Fanservice / Ecchi",
                    detail: "Exclude gratuitous sexualized tropes or bathhouse tropes",
                    symbol: "",
                    tags_pos: &[], tags_neg: &["Ecchi", "Harem"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "No Extreme Gore or Torture",
                    detail: "Exclude visceral mutilation, horror gore, or torture scenes",
                    symbol: "",
                    tags_pos: &[], tags_neg: &["Gore", "Horror"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "No Slow-Burn Pacing",
                    detail: "Exclude shows that take 5+ episodes to get moving",
                    symbol: "",
                    tags_pos: &[], tags_neg: &["Slow Paced"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "No High School Tropes",
                    detail: "Exclude high school romance, club drama, or teenage tropes",
                    symbol: "",
                    tags_pos: &[], tags_neg: &["School"],
                    min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
    ]
}

/// Dynamic 3 follow-up refinement questions branching specifically from the user's mood choice.
pub fn get_refinement_questions(q1_mood_choice: usize) -> Vec<TailoredQuestion> {
    let trope_options = match q1_mood_choice {
        // High Adrenaline & Hype
        0 => vec![
            TailoredOption {
                label: "Fluid Martial Arts & Close-Quarters Combat",
                detail: "Brutal hand-to-hand choreography, weapons mastery, visceral impact",
                symbol: "",
                tags_pos: &[("Martial Arts", 40.0), ("Hand-to-Hand Combat", 35.0), ("Super Power", 20.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Tactical Warfare & Army Strategy",
                detail: "Commanders, grand battalions, political conflict, battlefield maneuvers",
                symbol: "◈",
                tags_pos: &[("Military", 40.0), ("War", 35.0), ("Strategy", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Tournament Arcs & Ascendant Powers",
                detail: "Rankings, escalating power ceilings, intense tournament spectacles",
                symbol: "󰋽",
                tags_pos: &[("Tournament", 40.0), ("Super Power", 30.0), ("Action", 20.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Modern Supernatural & Shadow Assassins",
                detail: "Underground organizations, bounty hunters, gunplay, urban sorcery",
                symbol: "",
                tags_pos: &[("Urban Fantasy", 35.0), ("Assassins", 35.0), ("Gunplay", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
        ],
        // Deep & Mind-Bending
        1 => vec![
            TailoredOption {
                label: "Cat-and-Mouse Intellectual Mind Games",
                detail: "Geniuses predicting 10 steps ahead, high-stakes deception, duel of wits",
                symbol: "",
                tags_pos: &[("Mind Games", 45.0), ("Detective", 35.0), ("Conspiracy", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Time Loops, Paradoxes & Fractured Realities",
                detail: "Trying to alter fate, timelines collapsing, cause and effect puzzle",
                symbol: "◈",
                tags_pos: &[("Time Manipulation", 45.0), ("Time Loop", 40.0), ("Sci-Fi", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Existential Philosophy & Psychological Breakdown",
                detail: "What does it mean to be human? Questioning consciousness and reality",
                symbol: "󰋽",
                tags_pos: &[("Philosophy", 45.0), ("Psychological", 35.0), ("Dystopian", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Neo-Noir Investigation & Cyberpunk Crime",
                detail: "Rain-slicked streets, corrupt syndicates, tech intrigue, hardboiled mystery",
                symbol: "",
                tags_pos: &[("Noir", 40.0), ("Cyberpunk", 35.0), ("Crime", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
        ],
        // Cozy & Heartwarming
        2 => vec![
            TailoredOption {
                label: "Healing Iyashikei & Tranquil Nature",
                detail: "Quiet countryside, gentle pacing, stress-relieving meditative beauty",
                symbol: "",
                tags_pos: &[("Iyashikei", 45.0), ("Rural", 35.0), ("Outdoor", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Gourmet, Cafe & Culinary Delights",
                detail: "Mouthwatering dishes, heartwarming cooking, warm customer interactions",
                symbol: "◈",
                tags_pos: &[("Food", 45.0), ("Cooking", 40.0), ("Gourmet", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Found Family & Tender Companionship",
                detail: "Unlikely bonds, guardianship, wholesome warmth, mutual support",
                symbol: "󰋽",
                tags_pos: &[("Family Life", 45.0), ("Parenting", 40.0), ("Slice of Life", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Eccentric Character Banter & Gentle Comedy",
                detail: "Hilarious misunderstandings, lovable oddballs, lighthearted school days",
                symbol: "",
                tags_pos: &[("Comedy", 40.0), ("School", 30.0), ("Parody", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
        ],
        // Emotional Rollercoaster
        3 => vec![
            TailoredOption {
                label: "Heart-Wrenching Romantic Tragedy",
                detail: "Uncontrollable destiny, tearful departures, profound emotional catharsis",
                symbol: "",
                tags_pos: &[("Tragedy", 45.0), ("Melodrama", 40.0), ("Romance", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Coming-of-Age Nostalgia & Self-Discovery",
                detail: "Youth, melancholy, longing for the past, transitions into adulthood",
                symbol: "◈",
                tags_pos: &[("Coming of Age", 45.0), ("School Life", 35.0), ("Drama", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Bittersweet Supernatural Farewells",
                detail: "Spirits, crossing boundaries, unfinished business, emotional closure",
                symbol: "󰋽",
                tags_pos: &[("Afterlife", 45.0), ("Supernatural", 35.0), ("Drama", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Slow-Burn Love & Unspoken Yearning",
                detail: "Gentle romantic development, unspoken devotion, lingering glances",
                symbol: "",
                tags_pos: &[("Romance", 45.0), ("Love Triangle", 30.0), ("Drama", 25.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
        ],
        // Dark, Bleak & Gritty (or default)
        _ => vec![
            TailoredOption {
                label: "Vengeance Quest & Dark Fantasy Bloodshed",
                detail: "A wronged warrior, ruthless retribution, punishing moral grayness",
                symbol: "",
                tags_pos: &[("Revenge", 45.0), ("Dark Fantasy", 40.0), ("Gore", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Psychological Survival & Deadly Games",
                detail: "Hostile environments, elimination stakes, distrust among allies",
                symbol: "◈",
                tags_pos: &[("Survival", 45.0), ("Death Game", 40.0), ("Psychological", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Dystopian Tyranny, Rebellion & Cyber-Underworld",
                detail: "Authoritarian rule, cybernetics, high-tech oppression, desperate resistance",
                symbol: "󰋽",
                tags_pos: &[("Dystopian", 45.0), ("Cyberpunk", 40.0), ("Anti-Hero", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
            TailoredOption {
                label: "Eldritch Horrors & Demonic Nightmares",
                detail: "Inhuman monsters, body horror, dread of the unknown, cosmic terror",
                symbol: "",
                tags_pos: &[("Horror", 45.0), ("Monsters", 40.0), ("Demons", 30.0)],
                tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
            },
        ],
    };

    vec![
        TailoredQuestion {
            id: 101,
            title: "Narrative Trope & Plot Catalyst",
            subtitle: "Select the specific storyline mechanisms you crave (Select all that apply)",
            symbol: "󰋽",
            is_multi_select: true,
            options: trope_options,
        },
        TailoredQuestion {
            id: 102,
            title: "Pacing & Hook Velocity",
            subtitle: "How should the story deliver its narrative rhythm? (Select all that apply)",
            symbol: "",
            is_multi_select: true,
            options: vec![
                TailoredOption {
                    label: "Fast-Burn & Immediate Hook",
                    detail: "Grabs you from minute 1 of Episode 1; breathless tempo with constant momentum",
                    symbol: "",
                    tags_pos: &[("Fast Paced", 35.0), ("Action", 20.0), ("Thriller", 20.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Methodical Slow-Burn & Layered Lore",
                    detail: "Patient character growth, intricate mystery, monumental late-season payoff",
                    symbol: "◈",
                    tags_pos: &[("Slow Paced", 35.0), ("Mystery", 25.0), ("Philosophy", 20.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Episodic Precision & Vignettes",
                    detail: "Standalone adventures and mini-arcs united by an overarching mystery",
                    symbol: "󰋽",
                    tags_pos: &[("Episodic", 35.0), ("Adventure", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Continuous Arc Escalation",
                    detail: "Stakes doubling continuously every arc with zero filler episodes",
                    symbol: "",
                    tags_pos: &[("Survival", 30.0), ("Super Power", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 103,
            title: "Visual Art & Production Aesthetic",
            subtitle: "What visual direction and animation style do you want? (Select all that apply)",
            symbol: "◈",
            is_multi_select: true,
            options: vec![
                TailoredOption {
                    label: "Grounded Cinematic Realism & Gritty Lighting",
                    detail: "Mature linework, atmospheric shadows, cinematic framing (Wit, Mappa, Production I.G)",
                    symbol: "",
                    tags_pos: &[("Cinematic", 35.0), ("Seinen", 25.0), ("Noir", 20.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Hyper-Kinetic Sakuga & Stylized Fluidity",
                    detail: "Explosive frame-rates, vibrant color palettes, dynamic angles (Ufotable, Bones, Trigger)",
                    symbol: "◈",
                    tags_pos: &[("Sakuga", 40.0), ("Stylized", 25.0), ("Action", 20.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Painterly Landscapes & Scenic Visual Poetry",
                    detail: "Breathtaking backdrops, ethereal lighting, emotional stillness (Kyoto Animation, CloverWorks)",
                    symbol: "󰋽",
                    tags_pos: &[("Scenic", 35.0), ("Iyashikei", 30.0), ("Atmospheric", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Expressive Avant-Garde & Surrealism",
                    detail: "Abstract compositions, bold color theory, unique auteur vision (Shaft, Science SARU)",
                    symbol: "",
                    tags_pos: &[("Surrealism", 40.0), ("Psychological", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 104,
            title: "Emotional Aftertaste & Catharsis",
            subtitle: "What resonance should linger with you after the final credits roll?",
            symbol: "◈",
            is_multi_select: true,
            options: vec![
                TailoredOption {
                    label: "Triumphant Euphoria & Hype",
                    detail: "Unstoppable human resolve, fist-pumping victory, tearful triumph against all odds",
                    symbol: "",
                    tags_pos: &[("Inspiring", 40.0), ("Super Power", 25.0), ("Action", 20.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Melancholic Solitude & Quiet Reverie",
                    detail: "Poignant bittersweetness, quiet rain, philosophical contemplation of transience",
                    symbol: "◈",
                    tags_pos: &[("Melancholy", 40.0), ("Drama", 30.0), ("Philosophy", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Mind-Shattering Twist & Reality Fracture",
                    detail: "Twists that completely recontextualize every preceding scene; existential dizziness",
                    symbol: "󰋽",
                    tags_pos: &[("Plot Twist", 45.0), ("Mind Games", 35.0), ("Psychological", 30.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Bleak Existential Dread & Uncompromising Stakes",
                    detail: "No plot armor, irreversible tragedy, ruthless psychological survival",
                    symbol: "",
                    tags_pos: &[("Tragedy", 40.0), ("Survival", 35.0), ("Horror", 25.0)],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
        TailoredQuestion {
            id: 105,
            title: "Curation Rarity & Discovery Tier",
            subtitle: "Filter out generic algorithms — what discovery depth do you desire?",
            symbol: "",
            is_multi_select: true,
            options: vec![
                TailoredOption {
                    label: "Hidden Masterpieces Only (Anti-Generic)",
                    detail: "High-scoring critical darlings (78%+) that mainstream audiences overlooked",
                    symbol: "",
                    tags_pos: &[],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
                TailoredOption {
                    label: "Modern S-Tier Cult Classics (2014–2024)",
                    detail: "The greatest auteur achievements of the modern anime renaissance",
                    symbol: "◈",
                    tags_pos: &[],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: Some(2014), max_year: Some(2025), status: None,
                },
                TailoredOption {
                    label: "Vintage Golden Era Legends (Pre-2010)",
                    detail: "Hand-drawn masterworks from the golden age with unmatched soul and craft",
                    symbol: "󰋽",
                    tags_pos: &[],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: Some(2010), status: None,
                },
                TailoredOption {
                    label: "Apex Legendary Titans (All-Time Giants)",
                    detail: "Undisputed titans of the medium regardless of era or popularity",
                    symbol: "",
                    tags_pos: &[],
                    tags_neg: &[], min_episodes: None, max_episodes: None, min_year: None, max_year: None, status: None,
                },
            ],
        },
    ]
}

/// Calculate the dynamic Final Fantasy XIV inspired soul persona and dimensional gauges.
pub fn calculate_persona(answers: &TailoredAnswers) -> Persona {
    let mood_idx    = answers.choices[0]; // 0: Hype, 1: Deep, 2: Cozy, 3: Emotional, 4: Dark
    let setting_idx = answers.choices[2]; // 0: Fantasy, 1: Cyberpunk, 2: Modern, 3: Feudal, 4: Isekai
    let hero_idx    = answers.choices[3]; // 0: Mastermind, 1: Underdog, 2: Veteran, 3: Relatable, 4: Ensemble
    let dark_idx    = answers.choices[5]; // 0: Uplifting, 1: Balanced, 2: Gray, 3: Brutal
    let depth_idx   = answers.choices[8]; // 0: Popcorn, 1: Philosophy, 2: Politics, 3: Character

    // Compute 5 Attribute Meters (0..=100)
    let intellect = match (mood_idx, hero_idx, depth_idx) {
        (1, 0, _) => 100,
        (1, _, 1) | (_, 0, 1) => 90,
        (_, 0, _) | (1, _, _) => 80,
        (_, _, 2) => 75,
        _ => 45,
    };

    let lore = match (setting_idx, depth_idx) {
        (0, 2) | (1, 2) => 100,
        (0, _) | (1, _) | (_, 2) => 85,
        (3, _) | (4, _) => 70,
        _ => 40,
    };

    let emotion = match (mood_idx, depth_idx) {
        (3, 3) => 100,
        (3, _) | (_, 3) => 85,
        (2, _) => 75,
        _ => 40,
    };

    let adrenaline = match (mood_idx, hero_idx) {
        (0, 1) | (0, 2) => 100,
        (0, _) => 85,
        (_, 1) | (_, 2) => 70,
        _ => 35,
    };

    let darkness = match dark_idx {
        3 => 100,
        2 => 75,
        1 => 45,
        _ => 20,
    };

    // Derive clear viewer profile title and palette
    let (title, epithet, symbol, primary, accent, resonance) = match (setting_idx, mood_idx) {
        (1, 1) | (1, 0) => (
            "CYBERPUNK STRATEGIST",
            "Drawn to futuristic sci-fi, psychological thrillers, and tactical intrigue",
            "◈",
            Color::Rgb(0, 240, 255),    // Neon Cyan
            Color::Rgb(255, 0, 127),    // Hot Pink
            vec!["Cyberpunk", "Psychological", "Tactical Mind"],
        ),
        (0, 1) | (0, _) if depth_idx == 1 || depth_idx == 2 => (
            "LORE & MYSTERY ARCHIVIST",
            "Drawn to intricate worldbuilding, political intrigue, and deep lore",
            "◈",
            Color::Rgb(251, 191, 36),   // Star Amber
            Color::Rgb(189, 147, 249),  // Lavender Purple
            vec!["High Fantasy", "Worldbuilding", "Politics"],
        ),
        (0, 0) | (_, 0) if hero_idx == 1 => (
            "ACTION & UNDERDOG SEEKER",
            "Drawn to high-energy combat, perseverance, and spectacle action",
            "",
            Color::Rgb(255, 110, 50),   // Solar Orange
            Color::Rgb(255, 230, 0),    // Flame Gold
            vec!["Martial Arts", "Underdog Grit", "Action"],
        ),
        (_, 2) => (
            "COZY SLICE-OF-LIFE FAN",
            "Drawn to relaxing pacing, gentle atmospheres, and heartwarming comfort",
            "󰋜",
            Color::Rgb(80, 250, 123),   // Emerald Green
            Color::Rgb(125, 207, 255),  // Sky Blue
            vec!["Iyashikei", "Slice of Life", "Comfort"],
        ),
        (_, 4) | (_, _) if dark_idx >= 2 => (
            "DARK & MATURE REALIST",
            "Drawn to high-stakes survival, moral ambiguity, and mature drama",
            "◈",
            Color::Rgb(255, 65, 84),    // Blood Crimson
            Color::Rgb(160, 140, 190),  // Ash Slate
            vec!["Dark Fantasy", "Survival", "Moral Ambiguity"],
        ),
        (_, 3) => (
            "EMOTIONAL DRAMA SEEKER",
            "Drawn to deep character bonds, romance, and moving stories",
            "",
            Color::Rgb(255, 121, 198),  // Rose Quartz
            Color::Rgb(189, 147, 249),  // Lavender
            vec!["Drama", "Romance", "Character Study"],
        ),
        _ => (
            "ADVENTURE EXPLORER",
            "Drawn to grand journeys, discovery, and dynamic ensemble casts",
            "󰋽",
            Color::Rgb(189, 147, 249),  // Lavender Purple
            Color::Rgb(251, 191, 36),
            vec!["Adventure", "Discovery", "Ensemble Cast"],
        ),
    };

    Persona {
        title,
        epithet,
        symbol,
        primary_color: primary,
        accent_color: accent,
        gauges: [
            ("Intellect & Tactics", intellect),
            ("Atmosphere & Lore", lore),
            ("Emotional Depth", emotion),
            ("Combat & Adrenaline", adrenaline),
            ("Darkness & Stakes", darkness),
        ],
        resonance_tags: resonance,
    }
}

/// Detect whether an anime title is a sequel season (Season 2+, 2nd Season, Part 2, etc.)
/// so fresh discovery isn't cluttered with mid-series entries.
pub fn is_sequel_season(anime: &Anime) -> bool {
    let candidates = [
        anime.title_english.as_deref().unwrap_or(""),
        anime.title_romaji.as_str(),
    ];

    for raw in candidates {
        let lower = raw.trim().to_lowercase();
        if lower.is_empty() {
            continue;
        }
        let markers = [
            "season 2",
            "season 3",
            "season 4",
            "season 5",
            "season 6",
            "2nd season",
            "3rd season",
            "4th season",
            "5th season",
            "part 2",
            "part 3",
            "cour 2",
            "2nd cour",
            "final season",
            "the final chapters",
            ": season ",
        ];
        if markers.iter().any(|m| lower.contains(m)) {
            return true;
        }
        let suffixes = [
            " ii", " iii", " iv", " v", " s2", " s3", " s4", " 2", " 3", " 4",
        ];
        if suffixes.iter().any(|s| lower.ends_with(s)) {
            return true;
        }
    }

    false
}

/// Score candidates from the SQLite database cache and AniList API using tiered mood coherence.
pub fn score_anime(anime: &Anime, answers: &TailoredAnswers) -> Option<TailoredMatch> {
    let questions = get_questions();

    // 0. Watchability Gate: Exclude unreleased or zero-episode / music entries
    if anime.status.as_deref() == Some("NOT_YET_RELEASED") {
        return None;
    }
    if anime.format.as_deref() == Some("MUSIC") {
        return None;
    }
    if anime.episodes == Some(0) {
        return None;
    }

    // 1. Hard Exclusion Gate: Deal-breakers (Q10)
    let anime_genres = anime.genre_list();
    let genre_lower: HashSet<String> = anime_genres.iter().map(|g| g.to_lowercase()).collect();

    for &deal_idx in &answers.deal_breakers {
        if let Some(opt) = questions[9].options.get(deal_idx) {
            for &neg in opt.tags_neg {
                if genre_lower.contains(&neg.to_lowercase()) {
                    return None; // Hard veto!
                }
            }
        }
    }

    // 2. Length Filter (Q2)
    let q2_opt = &questions[1].options[answers.choices[1]];
    let ep_count = anime.episodes.unwrap_or(0);
    if let Some(min) = q2_opt.min_episodes {
        if ep_count > 0 && ep_count < min {
            return None;
        }
    }
    if let Some(max) = q2_opt.max_episodes {
        if ep_count > max {
            return None;
        }
    }

    // 3. Status Filter (Q8)
    let q8_opt = &questions[7].options[answers.choices[7]];
    if let Some(req_status) = q8_opt.status {
        if anime.status.as_deref() != Some(req_status) {
            return None;
        }
    }

    // 4. Era Filter (Q7)
    let q7_opt = &questions[6].options[answers.choices[6]];
    let year = anime.season_year.unwrap_or(2020);
    if let Some(min_yr) = q7_opt.min_year {
        if year < min_yr {
            return None;
        }
    }
    if let Some(max_yr) = q7_opt.max_year {
        if year > max_yr {
            return None;
        }
    }

    // 5. Tiered Weighted Tag Matching & Primary Mood Coherence
    let mood_idx = answers.choices[0];
    let q1_opt = &questions[0].options[mood_idx];
    let mut mood_weight = 0.0f32;
    let mut mood_hits = 0usize;
    let mut reasons = Vec::new();

    for &(tag, weight) in q1_opt.tags_pos {
        if genre_lower.contains(&tag.to_lowercase()) {
            mood_weight += weight;
            mood_hits += 1;
            if reasons.len() < 3 && !reasons.contains(&tag.to_string()) {
                reasons.push(tag.to_string());
            }
        }
    }

    // Extra Cozy anchors recognized on AniList
    if mood_idx == 2 {
        for extra in ["iyashikei", "cute girls doing cute things", "healing", "food", "cooking"] {
            if genre_lower.contains(extra) && mood_hits == 0 {
                mood_weight += 30.0;
                mood_hits += 1;
            }
        }
    }

    // Negative tag penalties across Questions 1..=9
    let mut neg_penalty = 0.0f32;
    for (q_idx, &choice_idx) in answers.choices.iter().enumerate() {
        if let Some(opt) = questions.get(q_idx).and_then(|q| q.options.get(choice_idx)) {
            for &neg in opt.tags_neg {
                if genre_lower.contains(&neg.to_lowercase()) {
                    neg_penalty += if q_idx == 0 { 35.0 } else { 20.0 };
                }
            }
        }
    }

    // Cross-Mood Clash Penalty: stop battle shounen from invading Cozy Slice-of-Life (and vice versa)
    let has_sol_or_iyashikei = genre_lower.contains("slice of life") || genre_lower.contains("iyashikei");
    let has_action_or_shounen = genre_lower.contains("action")
        || genre_lower.contains("shounen")
        || genre_lower.contains("battle")
        || genre_lower.contains("martial arts");
    let has_dark_harsh = genre_lower.contains("horror")
        || genre_lower.contains("gore")
        || genre_lower.contains("survival")
        || genre_lower.contains("death game");

    let mut clash_penalty = 0.0f32;
    match mood_idx {
        // Cozy & Heartwarming
        2 => {
            if !has_sol_or_iyashikei && (has_action_or_shounen || has_dark_harsh) {
                clash_penalty += 55.0;
            } else if !has_sol_or_iyashikei && mood_hits == 0 {
                clash_penalty += 30.0;
            }
        }
        // High Adrenaline & Hype
        0 => {
            if has_sol_or_iyashikei && !genre_lower.contains("action") && !genre_lower.contains("sports") {
                clash_penalty += 45.0;
            }
        }
        // Deep & Mind-Bending
        1 => {
            let has_mind = ["psychological", "mystery", "thriller", "philosophy", "detective", "sci-fi"]
                .iter()
                .any(|t| genre_lower.contains(*t));
            if !has_mind {
                clash_penalty += 38.0;
            }
        }
        // Emotional Rollercoaster
        3 => {
            let has_emo = ["drama", "romance", "tragedy", "psychological", "coming of age"]
                .iter()
                .any(|t| genre_lower.contains(*t));
            if !has_emo {
                clash_penalty += 32.0;
            }
        }
        // Dark, Bleak & Gritty
        _ => {
            let has_grim = ["horror", "dark fantasy", "psychological", "survival", "tragedy", "thriller"]
                .iter()
                .any(|t| genre_lower.contains(*t));
            if has_sol_or_iyashikei && !has_grim {
                clash_penalty += 50.0;
            } else if !has_grim {
                clash_penalty += 30.0;
            }
        }
    }

    // Exclude blatant mood clashes unless an explicit anchor overrides it later
    if clash_penalty >= 45.0 && !has_sol_or_iyashikei {
        return None;
    }

    // Secondary weighted tag alignment (Q2..=Q9)
    let mut sec_weight = 0.0f32;
    for (q_idx, &choice_idx) in answers.choices.iter().enumerate().skip(1) {
        if let Some(opt) = questions.get(q_idx).and_then(|q| q.options.get(choice_idx)) {
            for &(tag, weight) in opt.tags_pos {
                if genre_lower.contains(&tag.to_lowercase()) {
                    sec_weight += weight;
                    if reasons.len() < 3 && !reasons.contains(&tag.to_string()) {
                        reasons.push(tag.to_string());
                    }
                }
            }
        }
    }

    // Normalized tiered percentage (genuine spread across 70%..=98%)
    let coherence_base = if mood_hits >= 1 { 24.0 } else { 4.0 };
    let mood_primary_score = (mood_weight / 35.0).min(1.0) * 34.0;
    let mood_extra_score = ((mood_weight - 35.0).max(0.0) / 45.0).min(1.0) * 6.0;
    let sec_score = (sec_weight / 45.0).min(1.0) * 22.0;
    let quality_score = ((anime.score.unwrap_or(70) as f32 - 60.0).clamp(0.0, 35.0) / 35.0) * 12.0;

    let raw_pct = coherence_base + mood_primary_score + mood_extra_score + sec_score + quality_score
        - neg_penalty
        - clash_penalty;

    let max_cap = if mood_hits == 0 { 78.0 } else { 98.0 };
    if raw_pct < 56.0 {
        return None;
    }
    let match_percentage = raw_pct.clamp(64.0, max_cap).round() as u8;

    if reasons.is_empty() {
        if let Some(first_genre) = anime_genres.first() {
            reasons.push(first_genre.clone());
        }
        reasons.push(format!(" {} Score", anime.score.unwrap_or(80)));
    }

    Some(TailoredMatch {
        anime: anime.clone(),
        match_percentage,
        reason_tags: reasons,
    })
}

/// Extract the top dynamic genre/tag chips present across the scored candidate pool
/// so the user can boost (`+`) or hide (`-`) real tags present in their recommendations.
pub fn extract_tuner_tags(pool: &[TailoredMatch], answers: &TailoredAnswers) -> Vec<String> {
    use std::collections::HashMap;
    let questions = get_questions();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for m in pool.iter().take(60) {
        for g in m.anime.genre_list().into_iter().take(6) {
            *counts.entry(g).or_insert(0) += 1;
        }
    }

    // Seed mood tags first so key primary mood genres are always easy to toggle
    let mut ordered = Vec::new();
    if let Some(q1) = questions[0].options.get(answers.choices[0]) {
        for &(tag, _) in q1.tags_pos {
            if !ordered.iter().any(|t: &String| t.eq_ignore_ascii_case(tag)) {
                ordered.push(tag.to_string());
            }
        }
    }

    let mut freq: Vec<(String, usize)> = counts.into_iter().collect();
    freq.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    for (tag, _) in freq {
        if !ordered.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            ordered.push(tag);
        }
        if ordered.len() >= 9 {
            break;
        }
    }

    ordered.truncate(9);
    ordered
}

/// Instantaneously filter and re-rank the candidate pool according to the Live Vibe Tuner controls
/// and return up to 30 recommendations.
pub fn apply_live_tuner(
    pool: &[TailoredMatch],
    _answers: &TailoredAnswers,
    tuner: &LiveTunerConfig,
) -> Vec<TailoredMatch> {
    let mut filtered: Vec<TailoredMatch> = pool
        .iter()
        .filter_map(|m| {
            // 1. Dismissed shows (`[x]`)
            if tuner.dismissed_ids.contains(&m.anime.id) {
                return None;
            }

            // 2. Anchor anime itself is excluded so peers surface
            if let Some((anchor_id, _, _)) = &tuner.anchor_anime {
                if m.anime.id == *anchor_id {
                    return None;
                }
            }

            // 3. Sequel Guard
            if tuner.hide_sequels && is_sequel_season(&m.anime) {
                return None;
            }

            // 4. Finished Only toggle
            if tuner.finished_only && m.anime.status.as_deref() != Some("FINISHED") {
                return None;
            }

            // 5. Discovery Tier filters
            match tuner.discovery_tier {
                1 => {
                    // Hidden Gems: good quality under-the-radar picks (score >= 74)
                    if m.anime.score.unwrap_or(0) < 74 {
                        return None;
                    }
                }
                2 => {
                    // Acclaimed 82%+
                    if m.anime.score.unwrap_or(0) < 82 {
                        return None;
                    }
                }
                3 => {
                    // Modern 2022+
                    if m.anime.season_year.unwrap_or(0) < 2022 {
                        return None;
                    }
                }
                _ => {}
            }

            let genres = m.anime.genre_list();
            let genre_lower: HashSet<String> = genres.iter().map(|g| g.to_lowercase()).collect();

            // 6. Muted `-` tags
            for muted in &tuner.muted_tags {
                if genre_lower.contains(&muted.to_lowercase()) {
                    return None;
                }
            }

            // 7. Score adjustment from `+` Boosted tags, Hidden Gems weighting, and `[m]` Anchor similarity
            let mut pct = m.match_percentage as i32;
            let mut reasons = m.reason_tags.clone();

            for boosted in &tuner.boosted_tags {
                if genre_lower.contains(&boosted.to_lowercase()) {
                    pct += 8;
                    if !reasons.iter().any(|r| r.eq_ignore_ascii_case(boosted)) {
                        reasons.insert(0, format!("+{}", boosted));
                    }
                } else {
                    pct -= 6;
                }
            }

            // Hidden Gems subtly favors 75..85 gems over ultra-mainstream 90+ giants
            if tuner.discovery_tier == 1 {
                let sc = m.anime.score.unwrap_or(75);
                if (75..=84).contains(&sc) {
                    pct += 5;
                    if !reasons.iter().any(|r| r == "Hidden Gem") {
                        reasons.push("Hidden Gem".to_string());
                    }
                }
            }

            // Locked `[m]` "More Like This" anchor boost
            if let Some((_, ref anchor_title, ref anchor_genres)) = tuner.anchor_anime {
                if tuner.anchor_peer_ids.contains(&m.anime.id) {
                    pct += 16;
                    reasons.insert(0, format!("Similar to {}", anchor_title));
                } else {
                    let shared = anchor_genres
                        .iter()
                        .filter(|g| genre_lower.contains(&g.to_lowercase()))
                        .count();
                    if shared >= 2 {
                        pct += (shared as i32 * 3).min(12);
                    }
                }
            }

            reasons.truncate(3);

            Some(TailoredMatch {
                anime: m.anime.clone(),
                match_percentage: pct.clamp(60, 99) as u8,
                reason_tags: reasons,
            })
        })
        .collect();

    filtered.sort_by(|a, b| {
        b.match_percentage
            .cmp(&a.match_percentage)
            .then_with(|| b.anime.score.cmp(&a.anime.score))
    });

    filtered.truncate(30);
    filtered
}

/// Score candidates with additional weighting from the 6 Crucible refinement questions & Keystone Anchor.
pub fn score_anime_refined(
    anime: &Anime,
    answers: &TailoredAnswers,
    refinement: &TailoredRefinement,
    peer_ids: Option<&HashSet<i64>>,
) -> Option<TailoredMatch> {
    // If user anchored on this exact anime, exclude it (recommend peers, not the anchor itself)
    if let Some(anchor_id) = refinement.anchor_anime_id {
        if anime.id == anchor_id {
            return None;
        }
    }

    // Run base gates (deal-breakers, length, status, era)
    let base_match = score_anime(anime, answers)?;

    let refinement_questions = get_refinement_questions(answers.choices[0]);
    let anime_genres = anime.genre_list();
    let genre_lower: HashSet<String> = anime_genres.iter().map(|g| g.to_lowercase()).collect();

    let mut extra_score = 0.0f32;
    let mut refined_reasons = base_match.reason_tags;

    // Direct peer of Anchor show (+35 points!)
    if let Some(peers) = peer_ids {
        if peers.contains(&anime.id) {
            extra_score += 35.0;
            let anchor_name = refinement.anchor_title.as_deref().unwrap_or("Anchor");
            refined_reasons.insert(0, format!("Similar to {}", anchor_name));
        }
    }

    // 1. Refinement Tropes (Multi-select)
    if let Some(q) = refinement_questions.first() {
        for &idx in &refinement.trope_selections {
            if let Some(opt) = q.options.get(idx) {
                for &(tag, weight) in opt.tags_pos {
                    if genre_lower.contains(&tag.to_lowercase()) {
                        extra_score += weight * 1.2;
                        let badge = tag.to_string();
                        if !refined_reasons.contains(&badge) {
                            refined_reasons.push(badge);
                        }
                    }
                }
            }
        }
    }

    // 2. Refinement Pacing (Multi-select)
    if let Some(q) = refinement_questions.get(1) {
        for &idx in &refinement.pacing_selections {
            if let Some(opt) = q.options.get(idx) {
                for &(tag, weight) in opt.tags_pos {
                    if genre_lower.contains(&tag.to_lowercase()) {
                        extra_score += weight;
                        let badge = opt.label.split('&').next().unwrap_or(opt.label).trim().to_string();
                        if !refined_reasons.contains(&badge) && refined_reasons.len() < 4 {
                            refined_reasons.push(badge);
                        }
                    }
                }
            }
        }
    }

    // 3. Refinement Visual Aesthetic (Multi-select)
    if let Some(q) = refinement_questions.get(2) {
        for &idx in &refinement.visual_selections {
            if let Some(opt) = q.options.get(idx) {
                for &(tag, weight) in opt.tags_pos {
                    if genre_lower.contains(&tag.to_lowercase()) {
                        extra_score += weight;
                    }
                }
            }
        }
    }

    // 4. Refinement Aftertaste & Catharsis (Multi-select)
    if let Some(q) = refinement_questions.get(3) {
        for &idx in &refinement.aftertaste_selections {
            if let Some(opt) = q.options.get(idx) {
                for &(tag, weight) in opt.tags_pos {
                    if genre_lower.contains(&tag.to_lowercase()) {
                        extra_score += weight;
                    }
                }
            }
        }
    }

    // 5. Refinement Rarity & Anti-Generic Filter (Multi-select)
    if refinement.rarity_selections.contains(&0) {
        // Hidden Masterpieces Only: boost if score >= 78
        if anime.score.unwrap_or(0) >= 78 {
            extra_score += 15.0;
            refined_reasons.push("Hidden Gem".to_string());
        }
    }
    if refinement.rarity_selections.contains(&1) {
        // Modern S-Tier Cult Classics (2014-2024)
        if let Some(y) = anime.season_year {
            if (2014..=2025).contains(&y) {
                extra_score += 12.0;
                refined_reasons.push("Modern Hit".to_string());
            }
        }
    }
    if refinement.rarity_selections.contains(&2) {
        // Vintage Golden Era (Pre-2010)
        if let Some(y) = anime.season_year {
            if y <= 2010 {
                extra_score += 18.0;
                refined_reasons.push("Classic Era".to_string());
            }
        }
    }

    refined_reasons.truncate(4);

    let base_pct = base_match.match_percentage as f32;
    let new_pct = (base_pct + (extra_score * 0.4)).clamp(78.0, 99.0) as u8;

    Some(TailoredMatch {
        anime: anime.clone(),
        match_percentage: new_pct,
        reason_tags: refined_reasons,
    })
}

/// Build a rich 120-item scored candidate pool from AniList + local cache for instant live tuning.
pub async fn generate_tailored_pool_advanced(
    pool: &sqlx::SqlitePool,
    client: &crate::api::anilist::AniListClient,
    answers: &TailoredAnswers,
    refinement: Option<&TailoredRefinement>,
    now: i64,
) -> Vec<TailoredMatch> {
    let questions = get_questions();

    // 1. Collect all watched anime IDs to exclude them from recommendations
    let watched_ids = crate::db::user::get_all_watched_anime_ids(pool)
        .await
        .unwrap_or_default();

    // 2. Build clean Primary Mood + Setting tag queries for AniList
    let mut mood_tags_in: Vec<String> = Vec::new();
    let mut combined_tags_in: Vec<String> = Vec::new();
    let mut tags_not_in: Vec<String> = Vec::new();

    if let Some(opt) = questions[0].options.get(answers.choices[0]) {
        for &(tag, _) in opt.tags_pos {
            let s = tag.to_string();
            if !mood_tags_in.contains(&s) {
                mood_tags_in.push(s.clone());
            }
            if !combined_tags_in.contains(&s) {
                combined_tags_in.push(s);
            }
        }
        for &neg in opt.tags_neg {
            if !tags_not_in.contains(&neg.to_string()) {
                tags_not_in.push(neg.to_string());
            }
        }
    }

    // Setting tags (Q3) added only to the combined precision query (not diluting pure mood discovery)
    if let Some(opt) = questions.get(2).and_then(|q| q.options.get(answers.choices[2])) {
        for &(tag, _) in opt.tags_pos {
            let s = tag.to_string();
            if !combined_tags_in.contains(&s) {
                combined_tags_in.push(s);
            }
        }
    }

    // Negative tags from Deal-breakers (Q10)
    for &deal_idx in &answers.deal_breakers {
        if let Some(opt) = questions[9].options.get(deal_idx) {
            for &neg in opt.tags_neg {
                if !tags_not_in.contains(&neg.to_string()) {
                    tags_not_in.push(neg.to_string());
                }
            }
        }
    }

    // Also exclude explicit Action pollution on AniList when Cozy & Heartwarming is chosen
    if answers.choices[0] == 2 && !tags_not_in.iter().any(|t| t.eq_ignore_ascii_case("Horror")) {
        tags_not_in.push("Horror".to_string());
    }

    let mut candidate_anime: Vec<Anime> = Vec::new();
    let mut seen_ids: HashSet<i64> = watched_ids.iter().copied().collect();
    let mut peer_ids: HashSet<i64> = HashSet::new();

    // 3. If an explicit anchor anime is active via refinement, include peer recommendations
    if let Some(refine) = refinement {
        if let Some(anchor_id) = refine.anchor_anime_id {
            if let Ok(peers) = client.fetch_peer_recommendations(anchor_id, now).await {
                for p in peers {
                    peer_ids.insert(p.id);
                    let _ = crate::db::cache::upsert_anime(pool, &p).await;
                    if seen_ids.insert(p.id) {
                        candidate_anime.push(p);
                    }
                }
            }
        }
    }

    // 4. Query AniList concurrently: Query A (Primary Mood + Setting) & Query B (Pure Mood Pool for deep coverage)
    let (res_combined, res_mood) = tokio::join!(
        client.tailored_search(&combined_tags_in, &tags_not_in, Some(70), None, &watched_ids, now),
        client.tailored_search(&mood_tags_in, &tags_not_in, Some(72), None, &watched_ids, now),
    );

    for batch in [res_combined, res_mood].into_iter().flatten() {
        for a in batch {
            let _ = crate::db::cache::upsert_anime(pool, &a).await;
            if seen_ids.insert(a.id) {
                candidate_anime.push(a);
            }
        }
    }

    // 5. Also fetch high-scoring local cached anime
    let local_rows = sqlx::query_as::<_, Anime>(
        "SELECT id, title_english, title_romaji, title_native, description, episodes, status, season, season_year, score, format, genres, cover_url, cover_blob, has_dub, updated_at, next_airing_episode FROM anime ORDER BY score DESC LIMIT 350;"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for a in local_rows {
        if seen_ids.insert(a.id) {
            candidate_anime.push(a);
        }
    }

    // 6. Score candidates
    let mut matches = Vec::new();
    for anime in candidate_anime {
        let scored = match refinement {
            Some(refine) => score_anime_refined(&anime, answers, refine, Some(&peer_ids)),
            None => score_anime(&anime, answers),
        };
        if let Some(m) = scored {
            matches.push(m);
        }
    }

    // 7. Sort by match_percentage DESC, then score DESC
    matches.sort_by(|a, b| {
        b.match_percentage
            .cmp(&a.match_percentage)
            .then_with(|| b.anime.score.cmp(&a.anime.score))
    });

    matches.truncate(120);
    matches
}

/// Query both the local cache and AniList GraphQL and return the top 30 tailored recommendations.
#[allow(dead_code)]
pub async fn generate_tailored_matches_advanced(
    pool: &sqlx::SqlitePool,
    client: &crate::api::anilist::AniListClient,
    answers: &TailoredAnswers,
    refinement: Option<&TailoredRefinement>,
    now: i64,
) -> Vec<TailoredMatch> {
    let pool_matches = generate_tailored_pool_advanced(pool, client, answers, refinement, now).await;
    apply_live_tuner(&pool_matches, answers, &LiveTunerConfig::default())
}

/// Query the local cache and rank anime based on the user's tailored answers (top 30).
#[allow(dead_code)]
pub async fn generate_tailored_matches(
    pool: &sqlx::SqlitePool,
    answers: &TailoredAnswers,
) -> Vec<TailoredMatch> {
    let rows = sqlx::query_as::<_, Anime>(
        "SELECT id, title_english, title_romaji, title_native, description, episodes, status, season, season_year, score, format, genres, cover_url, cover_blob, has_dub, updated_at, next_airing_episode FROM anime ORDER BY score DESC LIMIT 300;"
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut matches = Vec::new();
    let mut seen_ids = HashSet::new();

    for anime in rows {
        if seen_ids.insert(anime.id) && !is_sequel_season(&anime) {
            if let Some(m) = score_anime(&anime, answers) {
                matches.push(m);
            }
        }
    }

    // Sort by match_percentage DESC, then score DESC
    matches.sort_by(|a, b| {
        b.match_percentage
            .cmp(&a.match_percentage)
            .then_with(|| b.anime.score.cmp(&a.anime.score))
    });

    matches.truncate(30);
    matches
}

/// Calculate Serious/Analytical vs. Action/Emotional tone balance.
pub fn calculate_polarity(persona: &Persona) -> (u8, u8, &'static str, &'static str) {
    let intellect = persona.gauges[0].1 as f32;
    let lore = persona.gauges[1].1 as f32;
    let emotion = persona.gauges[2].1 as f32;
    let combat = persona.gauges[3].1 as f32;
    let darkness = persona.gauges[4].1 as f32;

    let umbral_raw = (darkness * 0.40) + (intellect * 0.35) + (lore * 0.25);
    let astral_raw = (combat * 0.45) + (emotion * 0.35) + ((100.0 - darkness).max(0.0) * 0.20);
    let total = (umbral_raw + astral_raw).max(1.0);

    let umbral_pct = ((umbral_raw / total) * 100.0).clamp(15.0, 85.0).round() as u8;
    let astral_pct = 100 - umbral_pct;

    let aspect = if umbral_pct >= 65 {
        "Analytical & Dark"
    } else if umbral_pct <= 35 {
        "High-Energy & Bright"
    } else {
        "Balanced Tone"
    };

    let demographic = if darkness >= 70.0 || intellect >= 85.0 {
        "Seinen / Mature"
    } else if combat >= 75.0 {
        "Shonen / Action"
    } else if emotion >= 75.0 {
        "Josei / Drama"
    } else {
        "All-Audience Appeal"
    };

    (umbral_pct, astral_pct, aspect, demographic)
}

/// Dynamic qualitative descriptor for dimensional gauge meters.
pub fn get_dimension_descriptor(gauge_name: &str, value: u8) -> &'static str {
    match gauge_name {
        "Intellect & Tactics" => {
            if value >= 85 { "Mastermind Calculation" }
            else if value >= 70 { "Tactical Mind Games" }
            else if value >= 50 { "Intuitive Flow" }
            else { "Straightforward Instinct" }
        }
        "Atmosphere & Lore" => {
            if value >= 85 { "Epic Arcane Worldcraft" }
            else if value >= 70 { "Rich Setting & Lore" }
            else if value >= 50 { "Balanced Scenery" }
            else { "Focused Realm" }
        }
        "Emotional Depth" => {
            if value >= 85 { "Heart-Wrenching Catharsis" }
            else if value >= 70 { "Resonant & Moving" }
            else if value >= 50 { "Grounded Comradeship" }
            else { "Lighthearted Solace" }
        }
        "Combat & Adrenaline" => {
            if value >= 85 { "High-Octane Spectacle" }
            else if value >= 70 { "Kinetic Dynamism" }
            else if value >= 50 { "Measured Tension" }
            else { "Gentle Peaceful Flow" }
        }
        "Darkness & Stakes" => {
            if value >= 85 { "Brutal Severe Mortality" }
            else if value >= 70 { "Morally Gray Abyss" }
            else if value >= 45 { "Balanced Bittersweet" }
            else { "Pure Wholesome Solace" }
        }
        _ => "Harmonized",
    }
}

/// Type alias for FFXIV Primal Elemental affinities: (Fire/Lightning, Wind/Ice, Earth/Water, Dominant).
pub type PrimalElements = ((u8, &'static str), (u8, &'static str), (u8, &'static str), &'static str);

/// Compute core genre category affinities: (Action/Pacing, Mystery/Strategy, Worldbuilding/Comfort, Dominant).
pub fn get_primal_elements(persona: &Persona) -> PrimalElements {
    let int_val = persona.gauges[0].1 as f32;
    let lore_val = persona.gauges[1].1 as f32;
    let emo_val = persona.gauges[2].1 as f32;
    let combat_val = persona.gauges[3].1 as f32;
    let dark_val = persona.gauges[4].1 as f32;

    // Action & Spectacle
    let fire = ((combat_val * 0.70) + ((100.0 - dark_val).max(0.0) * 0.30)).clamp(15.0, 95.0).round() as u8;

    // Mystery & Strategy
    let wind = ((int_val * 0.65) + (dark_val * 0.35)).clamp(15.0, 95.0).round() as u8;

    // Worldbuilding & Comfort
    let earth = ((lore_val * 0.50) + (emo_val * 0.50)).clamp(15.0, 95.0).round() as u8;

    let dominant = if earth >= fire && earth >= wind {
        "Worldbuilding & Comfort (Story & Atmosphere)"
    } else if wind >= fire {
        "Mystery & Strategy (Tactical & Deep Lore)"
    } else {
        "Action & Spectacle (High-Energy Pacing)"
    };

    ((fire, "Action & Pacing"), (wind, "Mystery & Strategy"), (earth, "World & Comfort"), dominant)
}

/// Returns a compact active weight/filter pill for question q_idx.
pub fn get_question_active_pill(answers: &TailoredAnswers, q_idx: usize) -> &'static str {
    match q_idx {
        0 => match answers.choices[0] {
            0 => "[+Hype +Action]",
            1 => "[+MindGames +Psych]",
            2 => "[+Iyashikei +Comfort]",
            3 => "[+Drama +Tears]",
            _ => "[+Darkness +Survival]",
        },
        1 => match answers.choices[1] {
            0 => "[1-4 Episodes]",
            1 => "[10-13 Episodes]",
            2 => "[20-26 Episodes]",
            _ => "[50+ Episodes]",
        },
        2 => match answers.choices[2] {
            0 => "[+Fantasy +Arcane]",
            1 => "[+Cyberpunk +SciFi]",
            2 => "[+Modern Urban]",
            3 => "[+Feudal Samurai]",
            _ => "[+Isekai Rebirth]",
        },
        3 => match answers.choices[3] {
            0 => "[+Mastermind Lead]",
            1 => "[+Underdog +Grit]",
            2 => "[+Veteran Skill]",
            3 => "[+Relatable Norm]",
            _ => "[+Ensemble Synergy]",
        },
        4 => match answers.choices[4] {
            0 => "[+War & Factions]",
            1 => "[+Mind Games]",
            2 => "[+Romance +Drama]",
            3 => "[+Tournament Stakes]",
            _ => "[+Survival Horror]",
        },
        5 => match answers.choices[5] {
            0 => "[Noble Spirit]",
            1 => "[Bittersweet]",
            2 => "[Morally Gray]",
            _ => "[Severe Mortality]",
        },
        6 => match answers.choices[6] {
            0 => "[Modern 2020s]",
            1 => "[Golden 2010s]",
            2 => "[Classic 2000s]",
            3 => "[Retro 90s]",
            _ => "[All Archive Eras]",
        },
        7 => match answers.choices[7] {
            0 => "[Finished Only]",
            1 => "[Airing Now]",
            _ => "[Any Status]",
        },
        8 => match answers.choices[8] {
            0 => "[Popcorn Pacing]",
            1 => "[+Philosophy]",
            2 => "[+Geopolitics]",
            _ => "[+Psychology]",
        },
        9 => {
            if answers.deal_breakers.is_empty() {
                "[0 Active Vetoes]"
            } else if answers.deal_breakers.len() == 1 {
                "[1 Veto Active]"
            } else {
                "[Multi-Veto Active]"
            }
        }
        _ => "[Active]",
    }
}

/// Generate 8 dynamic preference badges from user answers.
pub fn get_tasting_notes(answers: &TailoredAnswers) -> Vec<&'static str> {
    let mut notes = Vec::new();

    // 1. Mood note
    match answers.choices[0] {
        0 => notes.push("High Adrenaline"),
        1 => notes.push("Tactical Mind Games"),
        2 => notes.push("Cozy & Relaxing"),
        3 => notes.push("Emotional Drama"),
        _ => notes.push("Dark & Intense"),
    }

    // 2. Setting note
    match answers.choices[2] {
        0 => notes.push("High Fantasy"),
        1 => notes.push("Cyberpunk & Sci-Fi"),
        2 => notes.push("Modern Urban"),
        3 => notes.push("Historical Samurai"),
        _ => notes.push("Isekai Adventure"),
    }

    // 3. Hero note
    match answers.choices[3] {
        0 => notes.push("Strategist Lead"),
        1 => notes.push("Underdog Growth"),
        2 => notes.push("Skilled Veteran"),
        3 => notes.push("Relatable Lead"),
        _ => notes.push("Ensemble Cast"),
    }

    // 4. Darkness note
    match answers.choices[5] {
        0 => notes.push("Uplifting Tone"),
        1 => notes.push("Balanced Tone"),
        2 => notes.push("Morally Gray"),
        _ => notes.push("High Stakes & Gritty"),
    }

    // 5. Depth note
    match answers.choices[8] {
        0 => notes.push("Fast Pacing"),
        1 => notes.push("Philosophical"),
        2 => notes.push("Political Intrigue"),
        _ => notes.push("Deep Psychology"),
    }

    // 6. Conflict note
    match answers.choices[4] {
        0 => notes.push("Large-Scale Wars"),
        1 => notes.push("Mystery & Intrigue"),
        2 => notes.push("Romance & Drama"),
        3 => notes.push("Tournaments"),
        _ => notes.push("Survival Stakes"),
    }

    // 7. Length / Deal-Breaker note
    if answers.deal_breakers.contains(&0) {
        notes.push("No Fanservice / Ecchi");
    } else if answers.deal_breakers.contains(&1) {
        notes.push("No Heavy Gore");
    } else if answers.deal_breakers.contains(&2) {
        notes.push("No Slow Pacing");
    } else {
        match answers.choices[1] {
            0 => notes.push("Short Mini-Series"),
            1 => notes.push("1-Cour (10-13 eps)"),
            2 => notes.push("2-Cour (20-26 eps)"),
            _ => notes.push("Long-Running (50+)"),
        }
    }

    // 8. Status / Canon note
    match answers.choices[7] {
        0 => notes.push("Finished Series"),
        1 => notes.push("Currently Airing"),
        _ => notes.push("Any Release Status"),
    }

    notes
}

/// Curated reference anime signatures with thematic descriptors.
pub fn get_spiritual_kinship_details(answers: &TailoredAnswers) -> Vec<(&'static str, &'static str)> {
    let mood_idx = answers.choices[0];
    let setting_idx = answers.choices[2];
    let dark_idx = answers.choices[5];

    if setting_idx == 1 || mood_idx == 1 {
        vec![
            ("Psycho-Pass", "Cyberpunk Noir"),
            ("Monster", "Moral Abyss"),
            ("Death Note", "Tactical Mind"),
            ("Ghost in the Shell", "Existential"),
        ]
    } else if setting_idx == 0 && dark_idx >= 2 {
        vec![
            ("Attack on Titan", "Severe Stakes"),
            ("Vinland Saga", "Redemption Arc"),
            ("Fate/Zero", "Grim Arcana"),
            ("86 Eighty-Six", "Sacrifice & War"),
        ]
    } else if mood_idx == 0 {
        vec![
            ("Jujutsu Kaisen", "Supernatural"),
            ("Hunter x Hunter", "Mastery & Hype"),
            ("Gurren Lagann", "Indomitable"),
            ("Chainsaw Man", "Visceral Action"),
        ]
    } else if mood_idx == 3 {
        vec![
            ("Steins;Gate", "Desperate Fate"),
            ("Clannad: After Story", "Family Love"),
            ("Violet Evergarden", "Remembrance"),
            ("Frieren", "Time & Memory"),
        ]
    } else if mood_idx == 2 {
        vec![
            ("Mushishi", "Atmospheric Solace"),
            ("Yuru Camp", "Campfire Peace"),
            ("Bocchi the Rock!", "Underdog Empathy"),
            ("Natsume Yuujinchou", "Healing Spirits"),
        ]
    } else {
        vec![
            ("Fullmetal Alchemist", "Equivalent Exchange"),
            ("Cowboy Bebop", "Space Jazz"),
            ("Code Geass", "Mastermind War"),
            ("Mob Psycho 100", "Heartfelt Growth"),
        ]
    }
}


/// Clean raw AniList descriptions of HTML tags, unescape entities, remove sources,
/// and prevent word concatenation across tag boundaries.
pub fn clean_synopsis(raw: &str) -> String {
    if raw.trim().is_empty() {
        return "No narrative synopsis recorded in archives.".to_string();
    }

    let text = raw.replace("\r\n", "\n").replace('\r', "\n");

    // 1. Decode standard HTML entities
    let text = text
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&nbsp;", " ");

    // 2. Parse tags and prevent word concatenation
    let mut raw_out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut prev_char: Option<char> = None;
    let mut crossed_inline_tag = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '<' {
            let mut end = i + 1;
            while end < chars.len() && chars[end] != '>' {
                end += 1;
            }
            let tag: String = chars[i + 1..end.min(chars.len())].iter().collect();
            let tag_lower = tag.trim().to_lowercase();

            if tag_lower == "br"
                || tag_lower.starts_with("br/")
                || tag_lower.starts_with("br ")
                || tag_lower == "br/"
            {
                raw_out.push('\n');
                prev_char = Some('\n');
                crossed_inline_tag = false;
            } else if tag_lower == "p"
                || tag_lower == "/p"
                || tag_lower.starts_with("p ")
                || tag_lower == "div"
                || tag_lower == "/div"
            {
                raw_out.push('\n');
                raw_out.push('\n');
                prev_char = Some('\n');
                crossed_inline_tag = false;
            } else {
                crossed_inline_tag = true;
            }
            i = (end + 1).min(chars.len());
        } else {
            // Strip markdown bold/italic markers
            if ch != '*' && ch != '_' {
                if crossed_inline_tag {
                    let prev_needs_space = prev_char
                        .map(|p| !p.is_whitespace() && p != '(' && p != '[' && p != '"' && p != '“')
                        .unwrap_or(false);
                    let next_needs_space = !ch.is_whitespace()
                        && !matches!(ch, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '"' | '”' | '\'');
                    if prev_needs_space && next_needs_space {
                        raw_out.push(' ');
                    }
                    crossed_inline_tag = false;
                }
                raw_out.push(ch);
                prev_char = Some(ch);
            }
            i += 1;
        }
    }

    // 2b. Ensure sentence-ending punctuation followed immediately by an uppercase word has a space
    let out_chars: Vec<char> = raw_out.chars().collect();
    let mut out = String::with_capacity(raw_out.len() + 16);
    for idx in 0..out_chars.len() {
        let c = out_chars[idx];
        out.push(c);
        if matches!(c, '.' | '!' | '?') && idx + 1 < out_chars.len() {
            let next_c = out_chars[idx + 1];
            let prev_is_lower = idx > 0 && out_chars[idx - 1].is_ascii_lowercase();
            if next_c.is_ascii_uppercase() && (c != '.' || prev_is_lower) {
                out.push(' ');
            }
        }
    }

    // 3. Filter out editorial source attributions
    let mut clean_lines = Vec::new();
    for line in out.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("(source:")
            || lower.starts_with("[source:")
            || lower.starts_with("(written by")
            || lower.starts_with("[written by")
            || lower.starts_with("<i>notes:")
            || lower.starts_with("notes:")
            || lower.starts_with("note:")
        {
            continue;
        }
        clean_lines.push(trimmed);
    }

    // 4. Collapse excessive blank lines
    let mut result = Vec::new();
    let mut blank_count = 0;
    for l in clean_lines {
        if l.is_empty() {
            blank_count += 1;
            if blank_count <= 1 && !result.is_empty() {
                result.push("");
            }
        } else {
            blank_count = 0;
            result.push(l);
        }
    }

    result.join("\n")
}

/// Clean a synopsis and collapse all line breaks and multi-spaces into a single flowing line
/// safe for `Span::styled` and `Paragraph` word-wrapping without concatenating words across `\n`.
pub fn clean_synopsis_single_line(raw: &str) -> String {
    let cleaned = clean_synopsis(raw);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate text cleanly at a word boundary without cutting words in half.
pub fn truncate_at_word_boundary(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars || max_chars <= 3 {
        return text.to_string();
    }
    let slice: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    if let Some(last_space) = slice.rfind(' ') {
        if last_space > max_chars / 2 {
            let trimmed = slice[..last_space]
                .trim_end_matches([',', ';', ':', '-', '·']);
            return format!("{}…", trimmed);
        }
    }
    format!("{}…", slice.trim_end())
}

/// Generate an engaging narrative hook/logline based on the anime's genres and tone.
pub fn get_thematic_hook(anime: &Anime) -> &'static str {
    let genres = anime.genre_list();
    let lower: Vec<String> = genres.iter().map(|g| g.to_lowercase()).collect();

    if lower.iter().any(|g| g == "psychological" || g == "mystery" || g == "thriller") {
        "A calculated descent into high-stakes moral dilemmas where intellect is weaponized."
    } else if lower.iter().any(|g| g == "sci-fi" || g == "mecha") {
        "In a neon dystopia where human soul and machine collide, truth demands sacrifice."
    } else if lower.iter().any(|g| g == "fantasy") && lower.iter().any(|g| g == "action" || g == "adventure") {
        "An odyssey through ancient perils where destiny and survival clash."
    } else if lower.iter().any(|g| g == "action" || g == "martial arts") {
        "Pulse-pounding adrenaline and ferocious combat where survival requires unyielding grit."
    } else if lower.iter().any(|g| g == "drama" || g == "romance") {
        "A moving exploration of sorrow, fragile connections, and lasting memories."
    } else if lower.iter().any(|g| g == "slice of life" || g == "comedy") {
        "Gentle warmth and lighthearted moments discovered in everyday life."
    } else {
        "A captivating journey through unforeseen peril, camaraderie, and triumph."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_questions_has_ten_questions() {
        let q = get_questions();
        assert_eq!(q.len(), 10);
        assert!(!q[0].is_multi_select);
        assert!(q[9].is_multi_select);
    }

    #[test]
    fn test_calculate_persona_allagan_archon() {
        let mut answers = TailoredAnswers::default();
        answers.choices[0] = 1; // Deep & Mind-Bending
        answers.choices[2] = 1; // Cyberpunk
        answers.choices[3] = 0; // Mastermind

        let persona = calculate_persona(&answers);
        assert!(persona.title.contains("CYBERPUNK STRATEGIST"));
        assert_eq!(persona.gauges[0].1, 100); // 100% intellect
    }

    #[test]
    fn test_calculate_persona_shadowbringer() {
        let mut answers = TailoredAnswers::default();
        answers.choices[5] = 3; // Brutal & Unforgiving

        let persona = calculate_persona(&answers);
        assert!(persona.title.contains("DARK & MATURE REALIST"));
        assert_eq!(persona.gauges[4].1, 100); // 100% darkness
    }

    #[test]
    fn test_score_anime_dealbreaker_exclusion() {
        let mut answers = TailoredAnswers::default();
        answers.deal_breakers.insert(0); // Exclude Ecchi

        let anime = Anime {
            genres: serde_json::to_string(&vec!["Action", "Ecchi"]).unwrap(),
            episodes: Some(12),
            ..Default::default()
        };

        let scored = score_anime(&anime, &answers);
        assert!(scored.is_none());
    }

    #[test]
    fn test_score_anime_positive_match() {
        let mut answers = TailoredAnswers::default();
        answers.choices[0] = 0; // High Adrenaline -> Action +35
        answers.choices[1] = 1; // 10-14 episodes
        answers.choices[2] = 0; // Fantasy -> Fantasy +40

        let anime = Anime {
            title_romaji: "Test Fantasy Action".to_string(),
            genres: serde_json::to_string(&vec!["Action", "Fantasy"]).unwrap(),
            episodes: Some(12),
            status: Some("FINISHED".to_string()),
            season_year: Some(2022),
            score: Some(88),
            ..Default::default()
        };

        let scored = score_anime(&anime, &answers);
        assert!(scored.is_some());
        let m = scored.unwrap();
        assert!(m.match_percentage >= 85);
        assert!(!m.reason_tags.is_empty());
    }

    #[test]
    fn test_calculate_polarity_umbral_bias() {
        let mut answers = TailoredAnswers::default();
        answers.choices[5] = 3; // Brutal Darkness
        answers.choices[0] = 1; // Deep Intellect
        let persona = calculate_persona(&answers);
        let (analytical, energetic, aspect, _) = calculate_polarity(&persona);
        assert!(analytical > energetic);
        assert!(aspect.contains("Analytical"));
    }

    #[test]
    fn test_get_tasting_notes_length() {
        let answers = TailoredAnswers::default();
        let notes = get_tasting_notes(&answers);
        assert_eq!(notes.len(), 8);
    }

    #[test]
    fn test_clean_synopsis_prevents_concatenation_and_cleans_tags() {
        let raw = "A boy finds a <i>Death Note</i>is dropped by a shinigami.<br><br>\r\nHe vows to change the world.<br>(Source: MAL)";
        let cleaned = clean_synopsis(raw);
        assert!(cleaned.contains("Death Note is dropped"));
        assert!(!cleaned.contains("<i>"));
        assert!(!cleaned.contains("(Source:"));
        assert!(cleaned.contains("He vows to change the world."));
    }

    #[test]
    fn test_get_refinement_questions_branches_per_mood() {
        // Mood 0 = Hype -> Martial Arts
        let q_hype = get_refinement_questions(0);
        assert_eq!(q_hype.len(), 5);
        assert!(q_hype[0].is_multi_select);
        assert!(q_hype[0].options[0].label.contains("Martial Arts"));

        // Mood 1 = Deep -> Mind Games
        let q_deep = get_refinement_questions(1);
        assert_eq!(q_deep.len(), 5);
        assert!(q_deep[0].options[0].label.contains("Mind Games"));

        // Mood 2 = Cozy -> Iyashikei
        let q_cozy = get_refinement_questions(2);
        assert_eq!(q_cozy.len(), 5);
        assert!(q_cozy[0].options[0].label.contains("Iyashikei"));
    }

    #[test]
    fn test_score_anime_refined_boosts_percentage_and_tags() {
        let mut answers = TailoredAnswers::default();
        answers.choices[0] = 1; // Deep & Mind-Bending
        answers.choices[1] = 1; // 10-14 episodes
        answers.choices[2] = 1; // Cyberpunk

        let anime = Anime {
            id: 999,
            title_romaji: "Ghost in the Machine".to_string(),
            genres: serde_json::to_string(&vec!["Psychological", "Sci-Fi", "Mind Games", "Fast Paced"]).unwrap(),
            episodes: Some(12),
            status: Some("FINISHED".to_string()),
            season_year: Some(2022),
            score: Some(86),
            ..Default::default()
        };

        let base_score = score_anime(&anime, &answers).unwrap();

        let mut trope_set = HashSet::new();
        trope_set.insert(0); // Mind Games
        let mut pacing_set = HashSet::new();
        pacing_set.insert(0); // Fast Paced

        let refinement = TailoredRefinement {
            trope_selections: trope_set,
            pacing_selections: pacing_set,
            visual_selections: HashSet::new(),
            aftertaste_selections: HashSet::new(),
            rarity_selections: HashSet::new(),
            anchor_anime_id: None,
            anchor_title: None,
        };

        let refined_score = score_anime_refined(&anime, &answers, &refinement, None).unwrap();
        assert!(refined_score.match_percentage >= base_score.match_percentage);
        assert!(refined_score.reason_tags.iter().any(|t| t.contains("Mind Games") || t.contains("Fast-Burn")));
    }

    #[test]
    fn test_score_anime_refined_peer_of_anchor_boost() {
        let mut answers = TailoredAnswers::default();
        answers.choices[1] = 2; // Full Season Arc (20-28 eps)
        let anime = Anime {
            id: 1575,
            title_romaji: "Code Geass".to_string(),
            genres: serde_json::to_string(&vec!["Action", "Mecha", "Super Power"]).unwrap(),
            episodes: Some(25),
            status: Some("FINISHED".to_string()),
            score: Some(85),
            ..Default::default()
        };

        let mut peers = HashSet::new();
        peers.insert(1575);

        let refinement = TailoredRefinement {
            trope_selections: HashSet::new(),
            pacing_selections: HashSet::new(),
            visual_selections: HashSet::new(),
            aftertaste_selections: HashSet::new(),
            rarity_selections: HashSet::new(),
            anchor_anime_id: Some(1535),
            anchor_title: Some("Death Note".to_string()),
        };

        let scored = score_anime_refined(&anime, &answers, &refinement, Some(&peers)).unwrap();
        assert!(scored.reason_tags.iter().any(|t| t.contains("Similar to Death Note")));
        assert!(scored.match_percentage >= 95);
    }

    #[test]
    fn test_cozy_mood_rejects_battle_shounen_and_rewards_slice_of_life() {
        let mut answers = TailoredAnswers::default();
        answers.choices[0] = 2; // Cozy & Heartwarming
        answers.choices[1] = 4; // Any Length
        answers.choices[2] = 0; // High Fantasy
        answers.choices[3] = 1; // Underdog
        answers.choices[5] = 1; // Balanced
        answers.choices[6] = 3; // Timeless
        answers.choices[7] = 2; // Any Status
        answers.choices[8] = 0; // Popcorn Fun

        let battle_shounen = Anime {
            id: 21,
            title_romaji: "Black Clover".to_string(),
            genres: serde_json::to_string(&vec!["Action", "Adventure", "Comedy", "Fantasy", "Shounen"]).unwrap(),
            episodes: Some(170),
            status: Some("FINISHED".to_string()),
            score: Some(81),
            ..Default::default()
        };

        let cozy_fantasy = Anime {
            id: 457,
            title_romaji: "Mushishi".to_string(),
            genres: serde_json::to_string(&vec!["Adventure", "Mystery", "Slice of Life", "Iyashikei", "Supernatural"]).unwrap(),
            episodes: Some(26),
            status: Some("FINISHED".to_string()),
            score: Some(87),
            ..Default::default()
        };

        assert!(score_anime(&battle_shounen, &answers).is_none());
        let cozy_score = score_anime(&cozy_fantasy, &answers).unwrap();
        assert!(cozy_score.match_percentage >= 84);
    }

    #[test]
    fn test_is_sequel_season_detects_sequels() {
        let s2 = Anime {
            title_english: Some("Black Clover Season 2".to_string()),
            title_romaji: "Black Clover Season 2".to_string(),
            ..Default::default()
        };
        let p2 = Anime {
            title_romaji: "Aoashi 2nd Season".to_string(),
            ..Default::default()
        };
        let s1 = Anime {
            title_english: Some("Frieren: Beyond Journey's End".to_string()),
            title_romaji: "Sousou no Frieren".to_string(),
            ..Default::default()
        };
        assert!(is_sequel_season(&s2));
        assert!(is_sequel_season(&p2));
        assert!(!is_sequel_season(&s1));
    }

    #[test]
    fn test_apply_live_tuner_returns_30_and_applies_filters() {
        let answers = TailoredAnswers::default();
        let mut pool = Vec::new();
        for i in 1..=45 {
            pool.push(TailoredMatch {
                anime: Anime {
                    id: i,
                    title_romaji: if i == 2 {
                        "Show Season 2".to_string()
                    } else {
                        format!("Sample Anime {}", i)
                    },
                    genres: serde_json::to_string(&if i == 5 {
                        vec!["Slice of Life", "Iyashikei"]
                    } else {
                        vec!["Slice of Life", "Comedy"]
                    }).unwrap(),
                    status: Some(if i % 2 == 0 { "RELEASING" } else { "FINISHED" }.to_string()),
                    season_year: Some(2023),
                    score: Some(85),
                    ..Default::default()
                },
                match_percentage: 86,
                reason_tags: vec!["Slice of Life".to_string()],
            });
        }

        let mut tuner = LiveTunerConfig::default();
        let default_res = apply_live_tuner(&pool, &answers, &tuner);
        assert_eq!(default_res.len(), 30);
        assert!(!default_res.iter().any(|m| m.anime.id == 2)); // Season 2 hidden

        tuner.boosted_tags.insert("Iyashikei".to_string());
        tuner.dismissed_ids.insert(1);
        let tuned_res = apply_live_tuner(&pool, &answers, &tuner);
        assert!(!tuned_res.iter().any(|m| m.anime.id == 1));
        assert_eq!(tuned_res[0].anime.id, 5); // Boosted Iyashikei moves to #1
    }
}
