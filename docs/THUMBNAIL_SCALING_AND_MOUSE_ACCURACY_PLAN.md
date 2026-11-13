# Solus Engineering Plan: 100% Frame-Accurate Thumbnail Scaling & Precision Mouse Navigation

## 1. Executive Summary & Problem Diagnosis

### Problem A: Thumbnail Deadspace & Frame Scaling Mismatch
Across different terminals (WezTerm, Windows Terminal, Kitty, Ghostty, Alacritty) and operating systems (Windows, macOS, Linux), cover art and episode thumbnails do not completely fill their designated UI frames. Users report:
- **Empty black borders (deadspace)** around thumbnails, where the rendered image is significantly smaller than the enclosing border or box.
- **Aspect Ratio Mismatch**: Modern anime poster images and landscape thumbnails (16:9) are scaled using `ratatui_image::Resize::Fit(None)`. When terminal fonts have non-standard aspect ratios (e.g., Cascadia Code at 11x24 px, Consolas at 10x21 px, or WezTerm line-height padding), `Resize::Fit` pillarboxes (shrinks width) or letterboxes (shrinks height) the image to preserve pixel aspect ratio.
- **Risk of Text Overlap**: Several frames contain UI text directly inside the frame (e.g., Home anime cards have Title + Score below the cover; Episode List cards have Episode Info to the right of the thumbnail). Any naïve stretching or overdrawing can obliterate this text. Dedicated frames without text (Featured Banner cover box, Detail Poster box, Split View Inspector Theater) must fill 100% of their inner geometry.

### Problem B: Mouse Click Offset & Inaccurate Selection
When users click an episode card in the Detail screen (e.g., clicking on Episode 8), Solus selects Episode 10 or 11 instead of Episode 8.
- **Root Cause**: In [src/main.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/main.rs#L1161-L1173), the mouse handler computed:
  ```rust
  let rel_ep_row = (my - 15) as usize;
  let target_ep = (state.episode_offset + rel_ep_row + 1) as u32;
  ```
  This assumed each episode card is **1 terminal row high**!
- **Reality**: In [src/ui/detail.rs](file:///Users/abdullahbinmadhi/Projects/anime/src/ui/detail.rs#L321), episode cards are dynamically **5, 6, or 7 rows high** (`card_height = if inner.height >= 21 { 7 } else if inner.height >= 14 { 6 } else { 5 };`), starting at `inner.y = 16`.
- Clicking at `y = 18` (middle of Episode 8) evaluated `(18 - 15) = 3`, incorrectly jumping 3 cards down to Episode 11.
- Furthermore, Home screen rows assumed `row_h = 9` (actual is `12` rows: 1 label + 10 card + 1 gap) and did not account for `start_idx` viewport scrolling.

---

## 2. Mathematical Root Cause Analysis

### 2.1 Terminal Cell Aspect Ratio Geometry
In terminal emulators, images are rendered inside character cell grids.
Let:
- $W_{cell}$ = Width of the target area in character cells (columns)
- $H_{cell}$ = Height of the target area in character cells (rows)
- $F_w$ = Font width in pixels per cell
- $F_h$ = Font height in pixels per cell

The physical pixel area rendered on screen is:
$$\text{Pixel Width} = W_{cell} \times F_w$$
$$\text{Pixel Height} = H_{cell} \times F_h$$
$$\text{Target Aspect Ratio} = \frac{W_{cell} \times F_w}{H_{cell} \times F_h}$$

### 2.2 Why `Resize::Fit` Creates Deadspace
`ratatui_image::Resize::Fit` takes the original image's cell size:
$$\text{desired.width} = \lceil \frac{\text{Image Width}}{F_w} \rceil, \quad \text{desired.height} = \lceil \frac{\text{Image Height}}{F_h} \rceil$$
And computes:
$$\text{ratio} = \min\left(\frac{W_{cell}}{\text{desired.width}}, \frac{H_{cell}}{\text{desired.height}}\right)$$
$$\text{nw} = \text{round}(\text{desired.width} \times \text{ratio})$$
$$\text{nh} = \text{round}(\text{desired.height} \times \text{ratio})$$

If the image was prepared with a hardcoded aspect ratio (such as `1280x768` = $1.6667$):
- On a terminal where $F_w = 11$, $F_h = 24$, a $20 \times 6$ cell card has a pixel aspect ratio of:
  $$\frac{20 \times 11}{6 \times 24} = \frac{220}{144} \approx 1.5278$$
- Since $1.5278 \neq 1.6667$, `Resize::Fit` fits the width ($nw = 20$), but height shrinks to $nh = 5$ rows!
- **Result**: Row 6 remains completely black/empty.
- For episode thumbnails of $17 \times 5$ cells, $nw$ shrinks to $13$ columns, leaving **4 columns of empty deadspace** between the thumbnail and the episode text.

---

## 3. Targeted Solution Architecture

### Component 1: `object-fit: cover` Dynamic Frame Scaler
Instead of hardcoding image dimensions to $1280 \times 768$, Solus will introduce a specialized, zero-deadspace scaler:
```rust
pub fn scale_to_frame_cover(
    img: DynamicImage,
    target_cols: u16,
    target_rows: u16,
    font_size: (u16, u16),
    upper_bias: f32,
) -> DynamicImage
```

#### The Algorithm:
1. **Compute Physical Target Dimensions**:
   $$P_w = \text{target\_cols} \times F_w$$
   $$P_h = \text{target\_rows} \times F_h$$
   To ensure maximum sharpness and prevent GPU blur, upscale by an integer factor $M \in [2, 4]$:
   $$\text{target\_w} = P_w \times M, \quad \text{target\_h} = P_h \times M$$
2. **Center-Weighted Upper-Biased Crop (Object-Fit: Cover)**:
   - Compare source image aspect ratio $(W_{src} / H_{src})$ with target aspect ratio $(P_w / P_h)$.
   - If source is narrower/taller than target: scale width to $\text{target\_w}$, then crop excess height using `upper_bias` (e.g. $0.18$ - $0.22$) to ensure anime character faces and eyes remain in focus.
   - If source is wider than target: scale height to $\text{target\_h}$, then center-crop excess width.
3. **Integer Multiplier Guarantee**:
   Because $\text{target\_w} = W_{cell} \times F_w \times M$ and $\text{target\_h} = H_{cell} \times F_h \times M$:
   $$\text{desired.width} = \frac{\text{target\_w}}{F_w} = W_{cell} \times M$$
   $$\text{desired.height} = \frac{\text{target\_h}}{F_h} = H_{cell} \times M$$
   $$\frac{\text{desired.width}}{\text{desired.height}} \equiv \frac{W_{cell}}{H_{cell}}$$
   `wratio` and `hratio` become mathematically identical down to the lowest bit:
   $$\text{wratio} = \frac{W_{cell}}{W_{cell} \times M} = \frac{1}{M}, \quad \text{hratio} = \frac{H_{cell}}{H_{cell} \times M} = \frac{1}{M}$$
   $$nw \equiv W_{cell}, \quad nh \equiv H_{cell}$$
   **Guaranteed: $0\%$ deadspace, 100% edge-to-edge fill, on every terminal, font, and OS.**

---

### Component 2: Container Type Inventory & Boundary Enforcement

| Container | Location | Target Cells ($W \times H$) | Text Inside Container? | Action & Boundary Protection |
| :--- | :--- | :--- | :--- | :--- |
| **Home Anime Card** | `src/ui/home.rs:586` | $20 \times 6$ | **Yes** (Title in `chunks[1]`, Score in `chunks[2]`) | Scale to $20 \times 6$. Image strictly confined to `chunks[0]`. Zero bleed into title/score. |
| **Home Featured Banner** | `src/ui/home.rs:241` | $20 \times 6$ | **No** (Dedicated cover box) | Scale to $20 \times 6$. 100% border-to-border fill. |
| **Detail Anime Poster** | `src/ui/detail.rs:111` | $16 \times 12$ | **No** (Dedicated poster box) | Scale to $16 \times 12$. Standard portrait poster fill. |
| **Episode List Thumbnail** | `src/ui/detail.rs:420` | `thumb_width` $\times$ `card_inner.height` ($17 \times 5$, $14 \times 4$, etc.) | **Yes** (Episode Info in `card_cols[1]`) | Dynamic calculation matching `card_cols[0]`. Zero bleed into `card_cols[1]`. |
| **Split View Inspector Theater** | `src/ui/detail.rs:737` | `thumb_w` $\times$ `max_thumb_h` | **No** (Dedicated 16:9 theater frame) | Scale to `thumb_w` $\times$ `max_thumb_h`. 100% frame fill. |

---

### Component 3: 100% Precision Mouse Hit-Testing Engine

#### A. Detail Screen: List View
1. **Vertical Origin**:
   - `chunks[0]` (Hint bar): $y = 0$
   - `chunks[1]` (Top info panel): $y = 1..14$
   - `chunks[2]` (Episode list outer block): $y = 15$
   - Outer block has `Borders::TOP`, so cards begin at $y_{start} = 16$.
2. **Dynamic Card Height**:
   $$H_{card} = \begin{cases} 7 & \text{if } H_{inner} \ge 21 \\ 6 & \text{if } H_{inner} \ge 14 \\ 5 & \text{otherwise} \end{cases}$$
3. **Card Hit Math**:
   ```rust
   if my >= 16 {
       let rel_y = my - 16;
       let card_idx = (rel_y / card_height) as usize;
       let visible_cards = (inner_height / card_height).max(1) as usize;
       if card_idx < visible_cards {
           let target_ep_idx = state.episode_offset + card_idx;
           if target_ep_idx < state.episode_list.len() {
               let target_ep = state.episode_list[target_ep_idx];
               if state.selected_episode == Some(target_ep) {
                   begin_playback_flow(state, pool, cfg, false, tx).await;
               } else {
                   state.selected_episode = Some(target_ep);
               }
               return true;
           }
       }
   }
   ```
4. **Behavior**:
   - Clicking any cell within Card 0 ($y \in [16, 16 + H_{card} - 1]$) reliably selects the top visible episode.
   - Clicking any cell within Card 1 selects the second episode.
   - Clicking an already-selected card immediately launches video playback.

#### B. Detail Screen: Split View
1. **Left Queue ($mx < 44$)**:
   - Outer block starts at $y = 15$, $q\_block$ rounded borders add $1$ top border.
   - Episode items begin at $y = 17$, with **$1$ row per episode**.
   ```rust
   if mx < 44 && my >= 17 {
       let row_idx = (my - 17) as usize;
       if row_idx < q_inner_height {
           let target_ep_idx = state.episode_offset + row_idx;
           if target_ep_idx < state.episode_list.len() {
               let target_ep = state.episode_list[target_ep_idx];
               if state.selected_episode == Some(target_ep) {
                   begin_playback_flow(state, pool, cfg, false, tx).await;
               } else {
                   state.selected_episode = Some(target_ep);
               }
               return true;
           }
       }
   }
   ```
2. **Right Inspector Theater ($mx \ge 44$)**:
   - Clicking on the 16:9 thumbnail theater or stream options in the inspector triggers stream launch.

#### C. Home Screen Categories & Cards
1. **Row Height**: `row_height = 1 + CARD_HEIGHT + 1 = 12` rows (1 label, 10 card, 1 gap).
2. **Row Scrolling Offset**: Account for `start_idx = active_idx.saturating_sub(visible_rows - 1)` so vertical scrolling doesn't shift hit targets.
3. **Card Pitch**: `card_pitch = CARD_WIDTH + CARD_GAP = 22 + 2 = 24` columns.

---

## 4. Implementation Steps & File Checklist

### Step 1: Image Scaler in `src/main.rs` & `src/ui/components/cover.rs`
- Implement `scale_to_frame_cover(img, target_cols, target_rows, font_size, upper_bias)`.
- Use this helper when generating:
  - `card_img` (Home card & featured banner: $20 \times 6$)
  - `portrait_img` (Detail poster: $16 \times 12$)
  - `episode_thumbnail` (Detail list & split view: dynamic $W \times H$)

### Step 2: Boundary Guarding in `src/ui/home.rs` & `src/ui/detail.rs`
- Enforce strict `Rect` constraints on `render_stateful_widget`.
- Verify that `chunks[0]` in `render_card` is never overlapped by text paragraphs.
- Verify that `card_cols[0]` in `render_netflix_list` never bleeds into `card_cols[1]`.

### Step 3: Mouse Hit-Testing Engine in `src/main.rs`
- Replace naive `my - 15` in `handle_mouse` with layout-aware `(my - 16) / card_height`.
- Support Split View left-column queue selection (`my - 17`).
- Align Home screen row math with `row_height = 12` and `start_idx`.

---

## 5. Verification & Testing Checklist

- [ ] **Unit Tests**:
  - Test `scale_to_frame_cover` with various non-square font geometries: $(11, 24)$, $(14, 28)$, $(10, 21)$, $(12, 24)$. Verify calculated `desired.width / desired.height` equals `target_cols / target_rows`.
  - Test mouse row-to-episode index mapping with `card_height = 5`, `6`, and `7`.
- [ ] **Terminal Emulators Verification**:
  - Windows Terminal (Cascadia Code)
  - WezTerm on Windows & macOS
  - Kitty & Ghostty on macOS
- [ ] **Text Non-Interference**:
  - Home card: Anime title and score are 100% visible, crisp, and untouched.
  - Detail card: Episode title, air date, and synopsis preview remain untouched.
- [ ] **Interactive Mouse Accuracy**:
  - Clicking on Episode 8 in List View selects Episode 8.
  - Double clicking Episode 8 triggers playback.
