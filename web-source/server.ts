import express from 'express';
import type { Request, Response } from 'express';
import { createServer as createViteServer } from 'vite';
import path from 'path';
import { fileURLToPath } from 'url';
import fs from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const PORT = parseInt(process.env.PORT || '3000', 10);

app.use(express.json());

// Guard against returning index.html for missing static assets (prevents syntax errors on old browsers)
app.use((req, res, next) => {
  const ext = path.extname(req.path).toLowerCase();
  const staticAssetExtensions = ['.js', '.mjs', '.css', '.json', '.png', '.jpg', '.jpeg', '.svg', '.webp', '.ico', '.woff', '.woff2', '.ttf', '.ttc'];
  if (staticAssetExtensions.includes(ext)) {
    if (!req.path.startsWith('/api') && !req.path.startsWith('/src') && !req.path.startsWith('/@') && !req.path.startsWith('/node_modules')) {
      const publicPath = path.resolve(__dirname, 'public', req.path.replace(/^\//, ''));
      const distPath = path.resolve(__dirname, 'dist', req.path.replace(/^\//, ''));
      if (!fs.existsSync(publicPath) && !fs.existsSync(distPath)) {
        return res.status(404).type('text/plain').send(`Not found: ${req.path}`);
      }
    }
  }
  next();
});

function decodeBase64Url(val: string): string {
  const pad = val + '='.repeat((4 - (val.length % 4)) % 4);
  const base64 = pad.replace(/-/g, '+').replace(/_/g, '/');
  return Buffer.from(base64, 'base64').toString('utf8');
}

function encodeBase64Url(val: string): string {
  return Buffer.from(val, 'utf8')
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

function isPermittedUrl(urlStr: string): boolean {
  try {
    const p = new URL(urlStr);
    if (!['http:', 'https:'].includes(p.protocol)) return false;
    const hostname = p.hostname.toLowerCase().replace(/\.$/, '');
    if (!hostname || hostname === 'localhost' || hostname === '127.0.0.1' || hostname.endsWith('.local') || hostname.endsWith('.internal')) {
      return false;
    }
    if (/^(127\.|10\.|192\.168\.|172\.(1[6-9]|2[0-9]|3[0-1])\.)/.test(hostname)) {
      return false;
    }
    return true;
  } catch {
    return false;
  }
}

function rewriteM3u8(text: string, baseUrl: string): string {
  const lines = text.split(/\r?\n/);
  const out: string[] = [];
  for (const line of lines) {
    const stripped = line.trim();
    if (stripped && !stripped.startsWith('#')) {
      try {
        const target = new URL(stripped, baseUrl).href;
        out.push('/api/proxy?u=' + encodeURIComponent(encodeBase64Url(target)));
      } catch {
        out.push(line);
      }
    } else if (stripped.startsWith('#')) {
      const replaced = line.replace(/(URI=")([^"]+)(")/gi, (_, p1, p2, p3) => {
        try {
          const target = new URL(p2, baseUrl).href;
          return `${p1}/api/proxy?u=${encodeURIComponent(encodeBase64Url(target))}${p3}`;
        } catch {
          return line;
        }
      });
      out.push(replaced);
    } else {
      out.push(line);
    }
  }
  return out.join('\n') + '\n';
}

// Default channels JSON
app.get('/api/channels/default', (_req: Request, res: Response) => {
  const filePath = path.resolve(__dirname, 'src/data/defaultChannels.json');
  if (fs.existsSync(filePath)) {
    res.setHeader('Content-Type', 'application/json; charset=utf-8');
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.sendFile(filePath);
  } else {
    res.status(404).json({ error: 'Default channels not yet generated' });
  }
});

// Load external playlist
app.get('/api/load', async (req: Request, res: Response) => {
  const targetUrl = (req.query.url as string) || '';
  if (!targetUrl || !isPermittedUrl(targetUrl)) {
    return res.status(403).send('Invalid or restricted target URL');
  }

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 20000);
    const upstream = await fetch(targetUrl, {
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36',
        Accept: '*/*',
      },
      signal: controller.signal,
    });
    clearTimeout(timeout);

    if (!upstream.ok) {
      return res.status(upstream.status).send(`Upstream returned ${upstream.status}`);
    }

    const text = await upstream.text();
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Content-Type', 'text/plain; charset=utf-8');
    res.setHeader('Cache-Control', 'no-store');
    return res.send(text);
  } catch (err: any) {
    return res.status(502).send('Error loading playlist: ' + (err?.message || 'unknown error'));
  }
});

// Speed test and resolution probe endpoint
app.get('/api/speedtest', async (req: Request, res: Response) => {
  const targetUrl = (req.query.url as string) || '';
  if (!targetUrl || !isPermittedUrl(targetUrl)) {
    return res.status(400).json({ ok: false, error: 'Invalid or restricted URL' });
  }

  const start = Date.now();
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 4000);
    const upstream = await fetch(targetUrl, {
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
        Accept: '*/*',
      },
      signal: controller.signal,
    });
    clearTimeout(timeout);
    const latency = Date.now() - start;

    if (!upstream.ok) {
      return res.json({ ok: false, latency, error: `HTTP ${upstream.status}` });
    }

    const text = await upstream.text();
    if (!text.includes('#EXTM3U')) {
      return res.json({ ok: false, latency, error: 'Not M3U8 stream' });
    }

    // Check resolution in master playlist
    const resMatches = [...text.matchAll(/RESOLUTION=(\d+)x(\d+)/g)];
    if (resMatches.length > 0) {
      const heights = resMatches.map((m) => parseInt(m[2], 10));
      const validHeights = heights.filter((h) => h >= 360 && h <= 720);
      const chosenHeight = validHeights.length > 0 ? Math.max(...validHeights) : heights[0];
      return res.json({
        ok: true,
        latency,
        height: chosenHeight,
        resolution: `${chosenHeight}P`,
        is360To720P: chosenHeight >= 360 && chosenHeight <= 720,
        availableResolutions: heights.map((h) => `${h}P`),
      });
    }

    // Try ffprobe
    try {
      const { stdout } = await execAsync(
        `ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of json -timeout 3500000 "${targetUrl}"`,
        { timeout: 4000 }
      );
      const parsed = JSON.parse(stdout);
      const h = parsed.streams?.[0]?.height;
      if (h) {
        return res.json({
          ok: true,
          latency,
          height: h,
          resolution: `${h}P`,
          is360To720P: h >= 360 && h <= 720,
        });
      }
    } catch {}

    return res.json({
      ok: true,
      latency,
      resolution: '未知画质',
      is360To720P: true,
    });
  } catch (err: any) {
    return res.json({
      ok: false,
      latency: Date.now() - start,
      error: err?.message || 'Connection failed',
    });
  }
});

// Proxy endpoint for stream playlist & segments
app.get('/api/proxy', async (req: Request, res: Response) => {
  const rawParam = (req.query.u as string) || '';
  if (!rawParam) {
    return res.status(400).send('Missing "u" query parameter');
  }

  let targetUrl = '';
  try {
    targetUrl = decodeBase64Url(rawParam);
  } catch {
    return res.status(400).send('Malformed base64 target');
  }

  if (!isPermittedUrl(targetUrl)) {
    return res.status(403).send('Restricted destination');
  }

  try {
    const headers: Record<string, string> = {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36',
      Accept: '*/*',
    };
    if (req.headers.range) {
      headers['Range'] = req.headers.range;
    }

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 18000);
    const upstream = await fetch(targetUrl, {
      headers,
      signal: controller.signal,
    });
    clearTimeout(timeout);

    const ctype = upstream.headers.get('content-type') || 'application/octet-stream';
    const isManifest =
      ctype.toLowerCase().includes('mpegurl') ||
      targetUrl.split('?')[0].toLowerCase().endsWith('.m3u8') ||
      ctype.toLowerCase().includes('apple.mpegurl');

    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Cache-Control', 'no-store');

    if (isManifest) {
      const rawText = await upstream.text();
      const rewritten = rewriteM3u8(rawText, upstream.url || targetUrl);
      res.setHeader('Content-Type', 'application/vnd.apple.mpegurl');
      return res.send(rewritten);
    }

    res.status(upstream.status);
    res.setHeader('Content-Type', ctype);

    for (const h of ['content-range', 'accept-ranges', 'content-length']) {
      const val = upstream.headers.get(h);
      if (val) res.setHeader(h, val);
    }

    if (!upstream.body) {
      return res.end();
    }

    const reader = upstream.body.getReader();
    const pump = async () => {
      try {
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          res.write(value);
        }
        res.end();
      } catch {
        res.end();
      }
    };
    return pump();
  } catch (err: any) {
    if (!res.headersSent) {
      return res.status(502).send('Proxy error: ' + (err?.message || 'unknown error'));
    }
    return res.end();
  }
});

app.get('/api/health', (_req, res) => {
  res.json({ status: 'ok', time: new Date().toISOString() });
});

// Explicitly serve web app manifest with standard Content-Type and CORS
app.get('/manifest.json', (_req: Request, res: Response) => {
  const isProd = process.env.NODE_ENV === 'production';
  const manifestPath = isProd
    ? path.resolve(__dirname, 'dist/manifest.json')
    : path.resolve(__dirname, 'public/manifest.json');
  const fallbackPath = path.resolve(__dirname, 'public/manifest.json');
  const target = fs.existsSync(manifestPath) ? manifestPath : fallbackPath;
  if (fs.existsSync(target)) {
    res.setHeader('Content-Type', 'application/manifest+json; charset=utf-8');
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Cache-Control', 'public, max-age=3600');
    return res.sendFile(target);
  }
  return res.status(404).type('text/plain').send('manifest.json not found');
});

async function startServer() {
  const isProd = process.env.NODE_ENV === 'production';
  if (!isProd) {
    const vite = await createViteServer({
      server: { middlewareMode: true },
      appType: 'spa',
    });
    app.use(vite.middlewares);
  } else {
    const distPath = path.resolve(__dirname, 'dist');
    // Cache static immutable assets (JS, CSS, images, fonts)
    app.use(
      '/assets',
      express.static(path.resolve(distPath, 'assets'), {
        maxAge: '1y',
        immutable: true,
      })
    );
    app.use(express.static(distPath));

    // SPA fallback: only for HTML page navigation, NEVER for missing assets
    app.get('*', (req, res) => {
      // If request has a file extension (e.g. .js, .css, .json, .png, etc.), return 404 text
      if (path.extname(req.path)) {
        return res.status(404).type('text/plain').send(`Not found: ${req.path}`);
      }
      res.setHeader('Cache-Control', 'no-cache, no-store, must-revalidate');
      res.setHeader('Pragma', 'no-cache');
      res.setHeader('Expires', '0');
      res.sendFile(path.resolve(distPath, 'index.html'));
    });
  }

  app.listen(PORT, '0.0.0.0', () => {
    console.log(`TouchIPTV Server running on http://0.0.0.0:${PORT}`);
  });
}

startServer();
