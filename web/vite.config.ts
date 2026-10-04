import { readdir, readFile, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { fileURLToPath, URL } from 'node:url'
import { brotliCompressSync, gzipSync } from 'node:zlib'

import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig, type Plugin } from 'vite'

/**
 * Writes a `.gz` and a `.br` copy of every text asset for the embedded server.
 * Runs on `closeBundle`: sources inside `generateBundle` still hold Vite's
 * `__VITE_PRELOAD__` placeholders, and compressing those broke the bundle.
 */
function precompress(): Plugin {
  const compressible = /\.(js|css|html|svg)$/
  let outDir = 'dist'

  async function walk(dir: string): Promise<string[]> {
    const entries = await readdir(dir, { withFileTypes: true })
    const files = await Promise.all(
      entries.map((entry) =>
        entry.isDirectory() ? walk(join(dir, entry.name)) : [join(dir, entry.name)],
      ),
    )
    return files.flat()
  }

  return {
    name: 'sailplane:precompress',
    apply: 'build',
    configResolved(config) {
      outDir = config.build.outDir
    },
    async closeBundle() {
      for (const file of await walk(resolve(outDir))) {
        if (!compressible.test(file)) continue

        const source = await readFile(file)
        // Below the block size the dictionaries cost more than they save.
        if (source.byteLength < 1024) continue

        for (const [suffix, encode] of [
          ['.gz', gzipSync],
          ['.br', brotliCompressSync],
        ] as const) {
          const compressed = encode(source)
          if (compressed.byteLength >= source.byteLength) continue
          await writeFile(`${file}${suffix}`, compressed)
        }
      }
    },
  }
}

export default defineConfig({
  // Relative asset URLs so the same build works at any mount point; the
  // server injects a matching <base href="…/"> into index.html.
  base: './',
  plugins: [vue(), tailwindcss(), precompress()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // Vite's own report would gzip every chunk again; `precompress` already
    // measures the real sizes.
    reportCompressedSize: false,
    rollupOptions: {
      output: {
        // Keep the framework in its own long-lived chunk so app updates do not
        // invalidate it in the browser cache. The match must be exact: `@vue`
        // also matches `@vue-flow`, which would drag the graph library into
        // every page instead of the one route that uses it.
        manualChunks(id) {
          if (/node_modules\/(vue|vue-router|vue-i18n|@vue)\//.test(id)) {
            return 'vendor'
          }
          return undefined
        },
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
      },
    },
  },
})
