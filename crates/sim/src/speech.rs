use crate::rng::SimRng;

/// Generates a line of gibberish from syllables, e.g. "Mubla zor-fi nuk!".
pub fn gibberish(rng: &mut SimRng, syllables: &[String], words: usize) -> String {
    if syllables.is_empty() {
        return "...".into();
    }
    let mut out = String::new();
    for w in 0..words.max(1) {
        if w > 0 {
            out.push(' ');
        }
        let n = 1 + rng.below(3);
        for s in 0..n {
            let syl = rng.pick(syllables).unwrap();
            if s > 0 && rng.chance(0.15) {
                out.push('-');
            }
            if w == 0 && s == 0 {
                let mut c = syl.chars();
                if let Some(f) = c.next() {
                    out.extend(f.to_uppercase());
                    out.push_str(c.as_str());
                }
            } else {
                out.push_str(syl);
            }
        }
    }
    out.push(*rng.pick(&['!', '?', '.', '~', '!']).unwrap());
    out
}

/// Replaces some longer words with [REDACTED] (Spy career flavour).
pub fn redact(rng: &mut SimRng, text: &str, chance: f32) -> String {
    text.split(' ')
        .map(|w| {
            let letters = w.chars().filter(|c| c.is_alphabetic()).count();
            if letters > 3 && !w.contains('{') && rng.chance(chance) { "[REDACTED]".to_string() } else { w.to_string() }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
