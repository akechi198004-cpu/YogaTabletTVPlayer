package com.gathin.touchiptv.data;

import android.content.Context;
import android.content.SharedPreferences;

import com.gathin.touchiptv.model.Channel;
import com.gathin.touchiptv.net.Net;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/** Owns the channel list: bundled playlist + optional user-imported playlist + preferences. */
public class ChannelRepository {

    private static final String PREFS = "touchiptv";
    private static final String KEY_FAV = "favorites";
    private static final String KEY_LAST = "last_channel";
    private static final String KEY_FILTER = "last_filter";
    private static final String KEY_ENGINE = "engine_mode";
    private static final String KEY_MUTED = "muted";
    private static final String KEY_VOLUME = "volume";
    private static final String KEY_SOURCE_URL = "source_url";
    private static final String CUSTOM_FILE = "custom_source.m3u";

    private final Context ctx;
    private final SharedPreferences sp;
    private Set<String> favorites = new HashSet<String>();

    public ChannelRepository(Context ctx) {
        this.ctx = ctx.getApplicationContext();
        this.sp = this.ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        favorites.addAll(sp.getStringSet(KEY_FAV, Collections.<String>emptySet()));
    }

    // ------------------------------------------------------------------ channels

    public List<Channel> loadChannels() {
        List<Channel> all = new ArrayList<Channel>();
        all.addAll(parseBundled());
        String custom = readFile(customFile());
        if (custom != null && custom.trim().length() > 0) {
            all = M3uParser.merge(all, M3uParser.parse(custom, "自定义源"));
        }
        for (Channel c : all) {
            c.favorite = favorites.contains(c.key());
        }
        return all;
    }

    private List<Channel> parseBundled() {
        List<Channel> out = new ArrayList<Channel>();
        InputStream in = null;
        try {
            in = ctx.getAssets().open("channels.json");
            ByteArrayOutputStream bos = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            while ((n = in.read(buf)) > 0) {
                bos.write(buf, 0, n);
            }
            JSONArray arr = new JSONArray(bos.toString("UTF-8"));
            for (int i = 0; i < arr.length(); i++) {
                JSONObject o = arr.optJSONObject(i);
                if (o == null) {
                    continue;
                }
                Channel c = new Channel();
                c.name = o.optString("name", "");
                c.id = o.optString("id", c.name);
                c.group = M3uParser.classifyGroup(c.name, o.optString("group", ""));
                c.url = o.optString("url", "");
                c.resolution = o.optString("resolution", "");
                c.height = o.optInt("height", 0);
                c.source = o.optString("source", "内置频道");
                JSONArray lines = o.optJSONArray("lines");
                if (lines != null) {
                    for (int k = 0; k < lines.length(); k++) {
                        String l = lines.optString(k, "");
                        if (l.length() > 0 && !c.lines.contains(l)) {
                            c.lines.add(l);
                        }
                    }
                }
                if (c.lines.isEmpty() && c.url.length() > 0) {
                    c.lines.add(c.url);
                }
                if (c.name.length() > 0 && !c.lines.isEmpty()) {
                    out.add(c);
                }
            }
        } catch (Throwable t) {
            // A broken asset must not crash the app; the list simply stays empty.
        } finally {
            Net.closeQuietly(in);
        }
        return out;
    }

    // ------------------------------------------------------------------ custom source

    public boolean hasCustomSource() {
        return sp.getString(KEY_SOURCE_URL, "").length() > 0;
    }

    public String customSourceUrl() {
        return sp.getString(KEY_SOURCE_URL, "");
    }

    public static final class ImportResult {
        public boolean ok;
        public int count;
        public String message = "";
    }

    /** Downloads and stores a playlist, replacing any previous custom source. Blocking: call off the UI thread. */
    public ImportResult importSource(String url) {
        ImportResult res = new ImportResult();
        String text = Net.getText(url, 15000, 6 * 1024 * 1024);
        if (text == null || text.trim().length() == 0) {
            res.message = "下载失败，请检查地址或网络";
            return res;
        }
        List<Channel> parsed = M3uParser.parse(text, "自定义源");
        if (parsed.isEmpty()) {
            res.message = "该地址不是有效的 M3U 播放列表";
            return res;
        }
        if (!writeFile(customFile(), text)) {
            res.message = "写入本地缓存失败";
            return res;
        }
        sp.edit().putString(KEY_SOURCE_URL, url).commit();
        res.ok = true;
        res.count = parsed.size();
        res.message = "已导入 " + parsed.size() + " 个频道";
        return res;
    }

    public void clearCustomSource() {
        File f = customFile();
        if (f.exists()) {
            //noinspection ResultOfMethodCallIgnored
            f.delete();
        }
        sp.edit().remove(KEY_SOURCE_URL).commit();
    }

    private File customFile() {
        return new File(ctx.getFilesDir(), CUSTOM_FILE);
    }

    // ------------------------------------------------------------------ preferences

    public boolean isFavorite(Channel c) {
        return c != null && favorites.contains(c.key());
    }

    public void toggleFavorite(Channel c) {
        if (c == null) {
            return;
        }
        String key = c.key();
        if (favorites.contains(key)) {
            favorites.remove(key);
            c.favorite = false;
        } else {
            favorites.add(key);
            c.favorite = true;
        }
        sp.edit().putStringSet(KEY_FAV, new HashSet<String>(favorites)).commit();
    }

    public int favoriteCount() {
        return favorites.size();
    }

    public String lastChannelName() {
        return sp.getString(KEY_LAST, "");
    }

    public void setLastChannelName(String name) {
        sp.edit().putString(KEY_LAST, name == null ? "" : name).commit();
    }

    public String lastFilter() {
        return sp.getString(KEY_FILTER, "全部");
    }

    public void setLastFilter(String filter) {
        sp.edit().putString(KEY_FILTER, filter).commit();
    }

    public int engineMode() {
        return sp.getInt(KEY_ENGINE, 0);
    }

    public void setEngineMode(int mode) {
        sp.edit().putInt(KEY_ENGINE, mode).commit();
    }

    public boolean isMuted() {
        return sp.getBoolean(KEY_MUTED, false);
    }

    public void setMuted(boolean muted) {
        sp.edit().putBoolean(KEY_MUTED, muted).commit();
    }

    public float volume() {
        return sp.getFloat(KEY_VOLUME, 1f);
    }

    public void setVolume(float v) {
        sp.edit().putFloat(KEY_VOLUME, v).commit();
    }

    // ------------------------------------------------------------------ io helpers

    private static String readFile(File f) {
        if (f == null || !f.exists()) {
            return null;
        }
        InputStream in = null;
        try {
            in = new FileInputStream(f);
            ByteArrayOutputStream bos = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            while ((n = in.read(buf)) > 0) {
                bos.write(buf, 0, n);
            }
            return bos.toString("UTF-8");
        } catch (Throwable t) {
            return null;
        } finally {
            Net.closeQuietly(in);
        }
    }

    private static boolean writeFile(File f, String text) {
        if (f == null || f.getParentFile() == null) {
            return false;
        }
        // Write beside the target and replace it only after the complete payload is on disk.
        // A failed import must leave the previous custom source intact instead of truncating it.
        File temp;
        try {
            temp = File.createTempFile(f.getName(), ".tmp", f.getParentFile());
        } catch (Throwable t) {
            return false;
        }
        FileOutputStream out = null;
        try {
            out = new FileOutputStream(temp);
            out.write(text.getBytes("UTF-8"));
            out.flush();
            out.close();
            out = null;
            if (!temp.renameTo(f)) {
                //noinspection ResultOfMethodCallIgnored
                temp.delete();
                return false;
            }
            return true;
        } catch (Throwable t) {
            //noinspection ResultOfMethodCallIgnored
            temp.delete();
            return false;
        } finally {
            if (out != null) {
                try {
                    out.close();
                } catch (Exception ignored) {
                }
            }
        }
    }
}
