package com.gathin.touchiptv.model;

import java.util.ArrayList;
import java.util.List;

/**
 * One live TV station. A station can carry several mirror streams ("线路"), played in order when
 * one of them stops answering.
 */
public class Channel {

    public String name = "";
    public String id = "";
    public String group = "";
    public String source = "";
    public String resolution = "";
    public int height;
    public int latency = -1;
    /** true once a speed test has produced {@link #latency} (-1 = timed out). */
    public boolean latencyKnown;
    public boolean favorite;

    /** First/preferred stream. Kept in sync with {@link #lines}. */
    public String url = "";

    public final List<String> lines = new ArrayList<String>();

    public Channel() {
    }

    public Channel(String name, String url) {
        this.name = name == null ? "" : name;
        this.id = this.name;
        this.url = url == null ? "" : url;
        if (this.url.length() > 0) {
            this.lines.add(this.url);
        }
    }

    public String line(int index) {
        if (lines.isEmpty()) {
            return url;
        }
        int i = Math.max(0, Math.min(index, lines.size() - 1));
        return lines.get(i);
    }

    public int lineCount() {
        return Math.max(1, lines.size());
    }

    /** Stable identity used for favourites and for merging duplicates from several playlists. */
    public String key() {
        String raw = (id != null && id.length() > 3) ? id : name;
        if (raw == null) {
            return "";
        }
        StringBuilder sb = new StringBuilder(raw.length());
        for (int i = 0; i < raw.length(); i++) {
            char c = Character.toLowerCase(raw.charAt(i));
            if (c == ' ' || c == '\u00b7' || c == '_' || c == '-' || c == '(' || c == ')'
                    || c == '[' || c == ']') {
                continue;
            }
            sb.append(c);
        }
        return sb.toString();
    }
}
