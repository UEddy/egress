import { loadFont } from '@remotion/fonts'
import { staticFile } from 'remotion'
import { FONT } from './theme'

// The same self hosted Inter the dashboard ships, one variable file covering every weight.
// Awaited at module scope so no frame can render with a fallback face.
await loadFont({
  family: FONT,
  url: staticFile('fonts/inter-latin-var.woff2'),
  weight: '100 900',
})
