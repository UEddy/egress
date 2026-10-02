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
- **fork-sim.mp4** is a recording of the output of `contracts/script/fork-netnet-aapl.sh`, a real
  run against a mainnet fork, rendered as a terminal and typed out line by line.

Both were captured with headless Chrome and encoded with the ffmpeg that ships with Remotion
(`npx remotion ffmpeg`). If the files are missing, the two scenes that embed them will fail to
render until they are recreated.

## Conventions

- Dark background, Inter, one accent colour, tabular numerals: the same tokens as
  `web/src/styles.css`, mirrored in `src/theme.ts`.
- Motion is spring based and nothing moves in under 12 frames, which is 0.4s at 30fps.
- Scenes cross dissolve over 15 frames. Without the overlap each scene opened on an empty frame
  while its first element faded up, which read as a flicker at every boundary.
