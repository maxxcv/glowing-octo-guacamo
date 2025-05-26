import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import ElementPlus from 'vite-plugin-element-plus';

export default defineConfig({
  plugins: [vue(), ElementPlus()],
    build: {
        sourcemap: true
          }
          });