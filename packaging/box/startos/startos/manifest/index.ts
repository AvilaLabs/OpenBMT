import { setupManifest } from '@start9labs/start-sdk'
import { long, short } from './i18n'

export const manifest = setupManifest({
  id: 'openbmt-box',
  title: 'OpenBMT Box',
  license: 'MIT',
  packageRepo: 'https://github.com/AvilaLabs/OpenBMT',
  upstreamRepo: 'https://github.com/AvilaLabs/OpenBMT',
  marketingUrl: 'https://github.com/AvilaLabs/OpenBMT',
  donationUrl: null,
  description: { short, long },
  volumes: ['main'],
  images: {
    box: {
      // The image is built from the Box's own Dockerfile. Both paths are
      // relative to this folder: the build context is the repository root.
      source: {
        dockerBuild: {
          workdir: '../../..',
          dockerfile: '../../Dockerfile',
        },
      },
      arch: ['x86_64', 'aarch64'],
    },
  },
})
