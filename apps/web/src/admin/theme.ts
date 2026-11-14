import { definePreset } from '@primeuix/themes'
import Aura from '@primeuix/themes/aura'

// docs/decisions.md 視覺風格. Surface: OKLCH hue 278°, chroma 0.028. Primary: hue 300°, chroma 0.13 clipped into
// sRGB, with 400 (Aura's dark primary color) set to the accent #d8b7ff.
export const theme = definePreset(Aura, {
  primitive: {
    // Flat, 6px corners everywhere.
    borderRadius: { lg: '6px', xl: '6px' },
  },
  semantic: {
    primary: {
      50: '#f6f3ff',
      100: '#eee7ff',
      200: '#e3d6fe',
      300: '#d9c6ff',
      400: '#d8b7ff',
      500: '#b191ea',
      600: '#9675cc',
      700: '#7e5db1',
      800: '#674698',
      900: '#512f7f',
      950: '#3a1463',
    },
    colorScheme: {
      dark: {
        surface: {
          0: '#ffffff',
          50: '#f9faff',
          100: '#f1f3ff',
          200: '#e0e3f8',
          300: '#cfd3e7',
          400: '#9b9fb2',
          500: '#6e7183',
          600: '#505364',
          700: '#3d3f50',
          800: '#252736',
          900: '#151725',
          950: '#070815',
        },
        formField: {
          // Aura's surface.600 is ~2.5:1 here; surface.500 is 3.7:1 on surface.900, 4.1:1 on surface.950 (WCAG 1.4.11).
          borderColor: '{surface.500}',
          hoverBorderColor: '{surface.400}',
          shadow: 'none',
        },
      },
    },
  },
})
