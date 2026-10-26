// SPDX-License-Identifier: GPL-3.0-or-later
// The Generate tab, without the screen: what is asked for (an illustration style that is picked from a list, and words for the
// character, its outfit and its body), how that becomes the text the picture AI is given, and the knobs of the AI (how many
// pictures, how closely it follows the words, how much detail, the size, the seed). The pose is always the same standard rig pose
// (arms a little away from the body, legs apart, seen from the front) so the 3D character moves in a game.

/// One illustration style: what the button says, the words it adds, and what it keeps out of the picture.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub label: &'static str,
    pub hint: &'static str,
    pub words: &'static str,
    pub avoid: &'static str,
}

const fn st(label: &'static str, hint: &'static str, words: &'static str, avoid: &'static str) -> Style {
    Style { label, hint, words, avoid }
}

/// The illustration styles.
pub const STYLES: &[Style] = &[
    st("Anime, clean", "Clean lines and flat cel shading", "anime style, clean lineart, cel shading, flat colors, vibrant", "photo, realistic, 3d render"),
    st("Anime, soft painted", "Anime with soft painted shading", "anime style, soft painted shading, detailed illustration, delicate colors", "photo, realistic, 3d render"),
    st("Game art", "Polished character art of a visual novel or a game", "visual novel character art, polished game illustration, detailed", "photo, 3d render"),
    st("Manga", "Ink lines and screentone, black and white", "manga style, ink lineart, black and white with screentone", "color photo, realistic"),
    st("Chibi", "Big head, small body, cute", "chibi, cute, big head, small body, simple shapes", "realistic proportions"),
    st("Cartoon", "Western cartoon with bold outlines", "western cartoon style, bold outlines, simple shading", "photo, realistic"),
    st("Stylized 3D", "A soft 3D look, like an animated film", "stylized 3d character render, soft shading, smooth materials", "lineart, photo"),
    st("Semi-realistic", "A detailed digital painting between drawing and photo", "semi-realistic digital painting, detailed, soft lighting", "cartoon, flat colors"),
    st("Realistic", "Looks like a photo of a person", "photorealistic, realistic skin, natural proportions, studio lighting, sharp focus", "cartoon, anime, drawing, painting, illustration"),
    st("Realistic 3D render", "A realistic game character render", "realistic 3d render, unreal engine character, detailed materials", "cartoon, anime, drawing"),
    st("Fantasy painting", "Painterly concept art with rich colors", "fantasy concept art, painterly brush strokes, rich colors, detailed", "photo, flat colors"),
    st("Watercolor", "Soft washes of color", "watercolor illustration, soft edges, light washes of color", "photo, 3d render"),
    st("Low poly", "Simple flat shapes, like an early 3D game", "low poly game character, flat shading, simple shapes", "photo, realistic"),
];

/// The sizes of the pictures: name, width and height.
pub const SIZES: &[(&str, u32, u32)] = &[("Tall", 768, 1344), ("Portrait", 832, 1216), ("Square", 1024, 1024)];

/// The pose of every character: the best one for a skeleton, whatever the style.
pub const POSE_WORDS: &str = "standing in an A-pose, arms slightly away from the body, legs slightly apart, seen from the front, symmetrical, centered, the whole body from head to feet visible with the shoes and a margin around, sharp detailed eyes, plain white background, no shadow on the ground, no text";

/// Everything that describes the picture that is asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// The place in `STYLES`.
    pub style: usize,
    /// Who it is, in words (a girl with long pink hair and green eyes).
    pub who: String,
    /// What it wears, in words.
    pub outfit: String,
    /// The body, in words (slim, tall, strong, short legs).
    pub body: String,
    /// Anything else (a red scarf, a calm look).
    pub more: String,
    /// What must not be there.
    pub avoid: String,
    pub size: usize,
    pub count: usize,
    pub steps: u32,
    /// How closely the picture follows the words (higher is stricter, lower is freer).
    pub guidance: f32,
    /// None picks a new one each time.
    pub seed: Option<u32>,
}

impl Default for Look {
    fn default() -> Self {
        Look { style: 0, who: String::new(), outfit: String::new(), body: String::new(), more: String::new(), avoid: String::new(), size: 0, count: 4, steps: 30, guidance: 6.5, seed: None }
    }
}

impl Look {
    pub fn size_px(&self) -> (u32, u32) {
        let s = SIZES[self.size.min(SIZES.len() - 1)];
        (s.1, s.2)
    }

    pub fn style(&self) -> &'static Style {
        &STYLES[self.style.min(STYLES.len() - 1)]
    }

    /// Is there anything written to make a picture from.
    pub fn has_words(&self) -> bool {
        [&self.who, &self.outfit, &self.body, &self.more].iter().any(|s| !s.trim().is_empty())
    }

    /// The text for the picture AI and the text for what must not be in it.
    pub fn prompt(&self) -> (String, String) {
        let st = self.style();
        let mut p: Vec<String> = vec!["full body character design".into(), st.words.into()];
        let who = self.who.trim();
        p.push(if who.is_empty() { "a character".into() } else { who.to_owned() });
        if !self.body.trim().is_empty() {
            p.push(format!("{} body", self.body.trim()));
        }
        if !self.outfit.trim().is_empty() {
            p.push(format!("wearing {}", self.outfit.trim()));
        }
        if !self.more.trim().is_empty() {
            p.push(self.more.trim().to_owned());
        }
        p.push(POSE_WORDS.into());
        let mut n: Vec<String> = vec![
            "blurry, low quality, deformed hands, extra limbs, extra fingers, cropped, cut off, text, watermark, signature, multiple characters, side view, back view, complex background, close-up, sitting, lying down, motion blur, ground shadow, floor, cast shadow, drop shadow".into(),
        ];
        if !st.avoid.is_empty() {
            n.push(st.avoid.into());
        }
        if !self.avoid.trim().is_empty() {
            n.push(self.avoid.trim().to_owned());
        }
        (p.join(", "), n.join(", "))
    }
}

/// What kind of game asset it is: the name on the button, the words for the picture, and the usual size (metres, the height).
#[derive(Clone, Copy, Debug)]
pub struct AssetKind {
    pub label: &'static str,
    pub words: &'static str,
    pub height_m: f32,
}

const fn ak(label: &'static str, words: &'static str, height_m: f32) -> AssetKind {
    AssetKind { label, words, height_m }
}

pub const ASSET_KINDS: &[AssetKind] = &[
    ak("Prop", "a game prop", 0.6),
    ak("Weapon", "a game weapon, shown upright", 1.0),
    ak("Tool", "a hand tool, shown upright", 0.7),
    ak("Furniture", "a piece of furniture", 1.0),
    ak("Container", "a chest or crate or barrel", 0.7),
    ak("Building piece", "a building or a part of a building", 4.0),
    ak("Vehicle", "a vehicle", 1.6),
    ak("Plant", "a plant or a tree", 2.0),
    ak("Rock", "a rock or a piece of terrain", 1.0),
    ak("Item", "a collectible game item", 0.3),
    ak("Food", "a food item", 0.2),
    ak("Armor piece", "a piece of armor on a stand", 0.7),
    ak("Magic item", "a magical artifact", 0.5),
    ak("Sci-fi device", "a sci-fi machine or device", 1.2),
    ak("Creature", "a small creature standing", 0.8),
];

/// The looks of a game asset.
pub const ASSET_STYLES: &[Style] = &[
    st("Hand painted", "Painted textures with soft shapes, like many fantasy games", "hand-painted stylized game asset, concept art illustration, painterly texture with visible brush strokes, simplified shapes, slightly exaggerated proportions, vibrant colors", "photo, photograph, photorealistic, realistic, 3d render, noisy"),
    st("Low poly", "Few flat faces and clean colors", "low poly game asset, illustration, flat shading, simple faceted shapes, clean flat colors", "photo, photograph, photorealistic, realistic, noisy, detailed texture"),
    st("Cartoon", "Bold shapes and clear colors", "cartoon game asset, illustration, bold simple shapes, thick outlines, clean saturated colors, toon shading", "photo, photograph, photorealistic, realistic"),
    st("Realistic", "Real materials and wear", "realistic game asset, detailed materials, subtle wear, even studio lighting", "cartoon, drawing"),
    st("Fantasy", "Ornate and magical", "fantasy game asset, concept art illustration, ornate details, rich painted materials", "photo, photograph, photorealistic"),
    st("Sci-fi", "Clean machines and glowing parts", "sci-fi game asset, hard surface, clean panels, glowing details", "photo, organic"),
    st("Stylized PBR", "Smooth realistic-ish materials with a stylized shape", "stylized pbr game asset, smooth materials, slightly exaggerated shapes, 3d render", "photo, photograph"),
    st("Pixel / voxel", "Chunky block look", "voxel style game asset, chunky blocks", "photo, smooth"),
];

/// A game asset that is asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetLook {
    pub kind: usize,
    pub style: usize,
    /// What it is, in words (a rusty iron lantern with a glass).
    pub what: String,
    /// Colours, materials, details.
    pub details: String,
    pub avoid: String,
    pub count: usize,
    pub steps: u32,
    pub guidance: f32,
    pub seed: Option<u32>,
    /// How tall it is in the game, in metres (the picked kind sets a first value).
    pub height_m: f32,
    /// What the game can afford.
    pub triangles: u32,
    /// The size of the texture picture.
    pub texture: u32,
    /// How fine the shape is made (192, 256 or 320).
    pub detail: u32,
    /// The model is held at the middle of its bottom (a prop on the floor) instead of its middle.
    pub pivot_bottom: bool,
}

impl Default for AssetLook {
    fn default() -> Self {
        AssetLook { kind: 0, style: 0, what: String::new(), details: String::new(), avoid: String::new(), count: 4, steps: 30, guidance: 7.0, seed: None, height_m: ASSET_KINDS[0].height_m, triangles: 8000, texture: 1024, detail: 256, pivot_bottom: true }
    }
}

impl AssetLook {
    pub fn kind(&self) -> &'static AssetKind {
        &ASSET_KINDS[self.kind.min(ASSET_KINDS.len() - 1)]
    }

    pub fn style(&self) -> &'static Style {
        &ASSET_STYLES[self.style.min(ASSET_STYLES.len() - 1)]
    }

    pub fn has_words(&self) -> bool {
        !self.what.trim().is_empty() || !self.details.trim().is_empty()
    }

    /// Pick a kind and take its usual size.
    pub fn set_kind(&mut self, i: usize) {
        self.kind = i.min(ASSET_KINDS.len() - 1);
        self.height_m = ASSET_KINDS[self.kind].height_m;
    }

    /// The text for the picture AI and the text for what must not be in it. One object, seen a little from above and from the side so
    /// the 3D step sees its shape, alone on white.
    pub fn prompt(&self) -> (String, String) {
        let (k, st) = (self.kind(), self.style());
        let mut p: Vec<String> = vec![k.words.into(), st.words.into()];
        if !self.what.trim().is_empty() {
            p.push(self.what.trim().to_owned());
        }
        if !self.details.trim().is_empty() {
            p.push(self.details.trim().to_owned());
        }
        p.push("one single object, isolated, three quarter view from slightly above, centered, the whole object visible, plain white background, no ground shadow, even soft lighting, no text".into());
        let mut n: Vec<String> = vec!["blurry, low quality, cropped, cut off, multiple objects, background scenery, hands holding it, a person, text, watermark, signature, dark shadows, motion blur".into()];
        if !st.avoid.is_empty() {
            n.push(st.avoid.into());
        }
        if !self.avoid.trim().is_empty() {
            n.push(self.avoid.trim().to_owned());
        }
        (p.join(", "), n.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_style_and_the_words_become_the_prompt() {
        let l = Look { style: 8, who: "a girl with long pink hair".into(), outfit: "a green coat".into(), body: "slim".into(), more: "a red scarf".into(), avoid: "freckles".into(), ..Default::default() };
        let (p, n) = l.prompt();
        assert!(p.contains("photorealistic") && p.contains("a girl with long pink hair"), "{p}");
        assert!(p.contains("slim body") && p.contains("wearing a green coat") && p.contains("a red scarf"), "{p}");
        assert!(n.contains("cartoon, anime") && n.contains("freckles"), "{n}");
    }

    #[test]
    fn the_pose_is_always_the_standard_rig_pose() {
        for style in 0..STYLES.len() {
            let (p, n) = Look { style, ..Default::default() }.prompt();
            assert!(p.contains("A-pose") && p.contains("from head to feet") && p.contains("white background"), "{p}");
            assert!(n.contains("side view"));
        }
    }

    #[test]
    fn nothing_written_still_gives_a_prompt_and_says_so() {
        let l = Look::default();
        assert!(!l.has_words());
        assert!(l.prompt().0.contains("a character"));
        assert!(Look { who: " a knight ".into(), ..Default::default() }.has_words());
    }

    #[test]
    fn a_game_asset_is_one_object_on_white_with_its_kind_and_style() {
        let mut a = AssetLook { what: "a rusty iron lantern".into(), details: "with cracked glass".into(), avoid: "chains".into(), style: 1, ..Default::default() };
        a.set_kind(1);
        let (p, n) = a.prompt();
        assert!(p.contains("a game weapon") && p.contains("low poly") && p.contains("rusty iron lantern") && p.contains("cracked glass"), "{p}");
        assert!(p.contains("one single object") && p.contains("white background"), "{p}");
        assert!(n.contains("multiple objects") && n.contains("chains"), "{n}");
        assert_eq!(a.height_m, 1.0, "the kind sets a usual size");
        assert!(a.has_words() && !AssetLook::default().has_words());
        let wild = AssetLook { kind: 99, style: 99, ..Default::default() };
        let _ = wild.prompt();
    }

    #[test]
    fn numbers_outside_the_lists_stay_inside() {
        let l = Look { style: 999, size: 99, ..Default::default() };
        let _ = l.prompt();
        assert_eq!(l.size_px(), (1024, 1024));
        assert!(STYLES.len() > 5);
    }
}
