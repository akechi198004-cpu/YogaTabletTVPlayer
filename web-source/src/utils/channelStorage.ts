import { ChannelItem } from './m3uParser';

const DB_NAME = 'touchiptv_db_v3';
const DB_VERSION = 1;
const STORE_NAME = 'channels_store';
const KEY = 'cached_channels_v3';

function openDB(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      return reject(new Error('IndexedDB not supported'));
    }
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function saveChannelsToStorage(channels: ChannelItem[]): Promise<void> {
  // 1. Always attempt IndexedDB first (no 5MB limit, handles 20,000+ channels easily)
  try {
    const db = await openDB();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readwrite');
      const store = tx.objectStore(STORE_NAME);
      const req = store.put(channels, KEY);
      req.onsuccess = () => resolve();
      req.onerror = () => reject(req.error);
    });
  } catch (err) {
    console.warn('IndexedDB save failed, falling back to localStorage', err);
  }

  // 2. Also try localStorage if smaller than ~1.5MB for quick instant sync
  try {
    if (channels.length <= 1500) {
      localStorage.setItem('touchiptv-channels', JSON.stringify(channels));
    }
  } catch {
    // If quota exceeded, IndexedDB already holds the complete dataset
  }
}

export async function loadChannelsFromStorage(): Promise<ChannelItem[] | null> {
  // 1. Try IndexedDB first
  try {
    const db = await openDB();
    const result = await new Promise<ChannelItem[] | null>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readonly');
      const store = tx.objectStore(STORE_NAME);
      const req = store.get(KEY);
      req.onsuccess = () => resolve(req.result || null);
      req.onerror = () => reject(req.error);
    });
    if (result && Array.isArray(result) && result.length > 0) {
      return result;
    }
  } catch (err) {
    console.warn('IndexedDB read failed, trying localStorage fallback', err);
  }

  // 2. Try localStorage fallback
  try {
    const raw = localStorage.getItem('touchiptv-channels');
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.length > 0) {
        return parsed;
      }
    }
  } catch {}

  return null;
}

export async function clearChannelsStorage(): Promise<void> {
  try {
    const db = await openDB();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readwrite');
      const store = tx.objectStore(STORE_NAME);
      const req = store.delete(KEY);
      req.onsuccess = () => resolve();
      req.onerror = () => reject(req.error);
    });
  } catch {}

  try {
    localStorage.removeItem('touchiptv-channels');
  } catch {}
}
