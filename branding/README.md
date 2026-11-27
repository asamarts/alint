# alint press kit

Brand assets for alint. Reuse these for write-ups, talks, package
listings, and social posts instead of screenshotting the site.

## Assets

| File | What | Use |
|---|---|---|
| `logo.svg` | Rounded-square mark, white lowercase "a" on alint indigo. 128 viewBox, reads down to 16px. | Favicon, avatar, anywhere the mark stands alone. |
| `logo-512.png` / `logo-256.png` / `logo-128.png` | Raster of the mark. | Package registries, GitHub org avatar, slide decks. |
| `social-card.svg` | 1200x630 Open Graph / Twitter card. Vector, tiny on the wire. | The on-site `og:image` source. |
| `social-card.png` | 1200x630 raster of the same card. | Link previews. Most scrapers (X, LinkedIn, Slack, Discord, Facebook) do not render an SVG `og:image`, so the raster is the one that actually shows up. |
| `demo.cast` | Legacy v0.6-era asciinema v2 recording (`check`, `config`, `list`, `suggest`, `fix`, re-check). | Archived historical asset; the README demo is now generated from `demo/alint.tape` via `demo/render.sh` into `demo/alint.gif`. |

## Brand

- Primary: `#4338ca` (indigo). Background gradient: `#0e0e10` to `#1e1b4b`.
- Accent text: `#a5b4fc`. Muted text: `#6b7280`.
- Wordmark: heavy weight, tight tracking. The SVGs name a system-UI
  font stack; a rasterizer without those fonts substitutes a clean
  sans, which is acceptable for the raster fallbacks here.

## License

Same dual license as the project (Apache-2.0 OR MIT). The marks
identify alint; do not modify them to imply a different project.

## Regenerate the raster

From the alint.org repo, with its pinned Node (`.nvmrc` = 24):

```sh
node -e "const s=require('sharp');for(const[i,o,w,h]of[['public/og-image.svg','public/og-image.png',1200,630],['public/favicon.svg','branding/logo-512.png',512,512]])s(i,{density:400}).resize(w,h,{fit:'fill'}).png().toFile(o)"
```

`sharp` requires Node >= 16; the system default may be older, so use
the nvm-pinned 24.

## Historical follow-ups (resolved)

1. **`og:image` PNG.** `social-card.png` is shipped alongside `social-card.svg` for scrapers that ignore SVG Open Graph previews.
2. **GitHub README demo.** Resolved: `README.md` embeds `demo/alint.gif`, generated reproducibly from `demo/alint.tape` via `demo/render.sh` and gated by `ci/scripts/demo-drift.sh`. `branding/demo.cast` is retained only as an archived v0.6 recording.
