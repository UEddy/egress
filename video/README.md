# Egress explainer video

A 2:30 Remotion video at 1920x1080, 30fps, rendered to `out/egress.mp4` in H.264.

Every number on screen is read from data at build time. Nothing is typed into a scene by hand.

- `../web/public/snapshot.json` is imported directly, the same committed file the dashboard renders
  from, so the video and the site can never disagree.
- `src/data/fork-result.json` is parsed from a real run of `contracts/script/fork-netnet-aapl.sh`.
- Derived rather than stated: the "63 of 63" readings figure is the pool count times the number of
  impact bounds in the snapshot, so it follows the data if the data changes.

`src/data.ts` is the only place that reads either file.

## Render

```bash
npm install
npm run render      # writes out/egress.mp4
npm run studio      # preview and scrub scenes
```

Each scene is also registered as its own composition, so a single scene can be previewed or
rendered on its own.

## The two clips

`public/live-check.mp4` and `public/fork-sim.mp4` are gitignored, because they are recordings
rather than source. Both are reproducible:

- **live-check.mp4** is a screen recording of the deployed dashboard at
  https://egress-theta.vercel.app, scrolled to the depth table, clicking Check live on AAPL. The
  call goes to the deployed engine over the public RPC, so the number that appears is live.
- **fork-sim.mp4** is a capture of an actual run of `contracts/script/fork-netnet-aapl.sh`. The
  session was recorded in a PTY with `script(1)`, which is the same mechanism asciinema uses,
  converted to asciicast v2, and replayed with asciinema-player. It is the real terminal output
  with its real timings, not a reconstruction. The run took 38.21s and plays at 4x, which the video
  states on screen along with the fork block and the path of the file holding the figures.

Both were captured with headless Chrome and encoded with the ffmpeg that ships with Remotion
(`npx remotion ffmpeg`). If the files are missing, the two scenes that embed them will fail to
render until they are recreated.

## Where the verification figure comes from

The "63 of 63" claim is read from `engine_verification` in
`tools/measure/results/engine-crosscheck-77749579.json`, written by:

```bash
cd tools/measure
cargo run --release -- verify-engine results/engine-crosscheck-77749579.json
```

That command asks the deployed engine every reading the file records, at the file's pinned block,
and writes back the matched and mismatched counts. The video quotes those counts, so the figure is
a record of a run rather than a count of how many readings there ought to be.

## Conventions

- Dark background, Inter, one accent colour, tabular numerals: the same tokens as
  `web/src/styles.css`, mirrored in `src/theme.ts`.
- Motion is spring based and nothing moves in under 12 frames, which is 0.4s at 30fps.
- Scenes cross dissolve over 15 frames. Without the overlap each scene opened on an empty frame
  while its first element faded up, which read as a flicker at every boundary.
