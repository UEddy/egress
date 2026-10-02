import { Composition, Folder } from 'remotion'
import './fonts'
import { EgressVideo, SCENES, TOTAL_FRAMES } from './EgressVideo'

export const RemotionRoot: React.FC = () => (
  <>
    <Composition
      id="EgressVideo"
      component={EgressVideo}
      durationInFrames={TOTAL_FRAMES}
      fps={30}
      width={1920}
      height={1080}
    />
    <Folder name="Scenes">
      {SCENES.map(({ name, durationInFrames, component }) => (
        <Composition
          key={name}
          id={name}
          component={component}
          durationInFrames={durationInFrames}
          fps={30}
          width={1920}
          height={1080}
        />
      ))}
    </Folder>
  </>
)
