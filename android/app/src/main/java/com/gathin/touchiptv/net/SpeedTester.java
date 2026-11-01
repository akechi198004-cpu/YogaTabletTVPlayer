package com.gathin.touchiptv.net;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Measures how fast a stream answers: connect + first bytes of the playlist. */
public final class SpeedTester {

    private static final Pattern RESOLUTION = Pattern.compile("RESOLUTION=\\d+x(\\d+)", Pattern.CASE_INSENSITIVE);
    private static final Pattern BANDWIDTH = Pattern.compile("BANDWIDTH=(\\d+)", Pattern.CASE_INSENSITIVE);

    public static final class Result {
        public boolean ok;
        public int latency = -1;
        public int height;
        public String resolution = "";
        public String error = "";
    }

    private SpeedTester() {
    }

    public static Result test(String url) {
        Result r = new Result();
        if (url == null || url.length() == 0) {
            r.error = "空地址";
            return r;
        }
        HttpURLConnection c = null;
        InputStream in = null;
        long start = System.currentTimeMillis();
        try {
            c = Net.open(url, 5000, 6000);
            c.setRequestProperty("Range", "bytes=0-65535");
            int code = c.getResponseCode();
            if (code < 200 || code >= 400) {
                r.error = "HTTP " + code;
                return r;
            }
            in = c.getInputStream();
            byte[] buf = new byte[4096];
            int n = in.read(buf);
            r.latency = (int) (System.currentTimeMillis() - start);
            if (n > 0) {
                String head = new String(buf, 0, n, "UTF-8");
                Matcher m = RESOLUTION.matcher(head);
                int best = 0;
                while (m.find()) {
                    int h = parseInt(m.group(1));
                    if (h > best) {
                        best = h;
                    }
                }
                if (best > 0) {
                    r.height = best;
                    r.resolution = best + "P";
                }
            }
            r.ok = true;
            return r;
        } catch (Throwable t) {
            r.error = t.getClass().getSimpleName();
            if (r.latency < 0) {
                r.latency = -1;
            }
            return r;
        } finally {
            Net.closeQuietly(in);
            if (c != null) {
                c.disconnect();
            }
        }
    }

    private static int parseInt(String s) {
        try {
            return Integer.parseInt(s);
        } catch (NumberFormatException e) {
            return 0;
        }
    }
}
