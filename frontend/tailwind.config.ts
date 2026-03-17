import type { Config } from 'tailwindcss'

export default {
  content: ['./index.html', './src/**/*.{vue,ts}'],
  theme: {
    extend: {
      colors: {
        bg0: '#1d2021',
        bg1: '#282828',
        bg2: '#3c3836',
        bg3: '#504945',
        bg4: '#665c54',
        fg0: '#fbf1c7',
        fg1: '#ebdbb2',
        fg2: '#d5c4a1',
        fg3: '#bdae93',
        muted: '#928374',
        red: '#fb4934',
        green: '#b8bb26',
        yellow: '#fabd2f',
        blue: '#83a598',
        purple: '#d3869b',
        aqua: '#8ec07c',
        orange: '#fe8019',
      },
      boxShadow: {
        panel: '0 0 0 1px rgba(251, 241, 199, 0.08), 0 22px 48px rgba(0, 0, 0, 0.38)',
      },
      fontFamily: {
        display: ['IBM Plex Mono', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', 'Courier New', 'monospace'],
        body: ['IBM Plex Mono', 'SFMono-Regular', 'Menlo', 'Monaco', 'Consolas', 'Liberation Mono', 'Courier New', 'monospace'],
      },
    },
  },
  plugins: [],
} satisfies Config
