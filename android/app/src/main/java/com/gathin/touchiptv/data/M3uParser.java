package com.gathin.touchiptv.data;

import com.gathin.touchiptv.model.Channel;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * M3U/M3U8 playlist reader, ported one-to-one from the web app's src/utils/m3uParser.ts so that a
 * playlist imported on the tablet lands in exactly the same categories the browser version showed.
 */
public final class M3uParser {

    private static final String[] KNOWN_GROUPS = {
            "央视", "卫视", "苏州", "香港", "台湾", "日本", "国际", "纪录", "少儿", "测试"
    };

    private M3uParser() {
    }

    public static String classifyGroup(String name, String group) {
        String g = group == null ? "" : group;
        for (String known : KNOWN_GROUPS) {
            if (known.equals(g)) {
                return known;
            }
        }
        String t = ((name == null ? "" : name) + " " + g).toLowerCase();
        if (containsAny(t, "cctv", "央视", "中央")) return "央视";
        if (containsAny(t, "卫视", "湖南", "浙江", "江苏", "北京", "东方", "深圳", "广东", "山东",
                "安徽", "湖北", "四川", "天津", "河南", "江西", "辽宁", "河北", "黑龙江", "吉林",
                "重庆", "三沙", "东南", "厦门", "宁夏", "山西")) return "卫视";
        if (t.contains("苏州")) return "苏州";
        if (containsAny(t, "tvb", "翡翠", "黄金华剧", "美亚", "凤凰", "香港")) return "香港";
        if (containsAny(t, "大立", "人间卫视", "tvbs", "台湾")) return "台湾";
        if (containsAny(t, "nhk", "weathernews", "mbs", "日本")) return "日本";
        if (containsAny(t, "cnn", "bbc news", "fox", "cna", "nbc", "dw", "cnbc", "sky news",
                "scripps", "brics", "reuters", "usa today", "半岛", "国际")) return "国际";
        if (containsAny(t, "earth", "nature", "documentary", "纪录", "cgtn documentary", "wonder", "hit")) {
            return "纪录";
        }
        if (containsAny(t, "少儿", "卡通", "animation", "cartoon", "cctv-14")) return "少儿";
        if (containsAny(t, "体育", "sport", "espn", "nba", "五星体育", "奥林匹克", "高尔夫", "cctv-5", "cctv-16")) {
            return "体育";
        }
        if (containsAny(t, "电影", "电视剧", "影视", "movie", "cinema", "剧场", "影院", "chc", "怀旧", "cctv-6", "cctv-8")) {
            return "影视";
        }
        if (containsAny(t, "新闻", "news", "财经", "business", "社会与法", "cctv-13", "cctv-2")) return "新闻";
        return g.length() > 0 ? g : "其他";
    }

    private static boolean containsAny(String haystack, String... needles) {
        for (String n : needles) {
            if (haystack.contains(n)) {
                return true;
            }
        }
        return false;
    }

    public static List<Channel> parse(String text, String sourceName) {
        List<Channel> result = new ArrayList<Channel>();
        if (text == null) {
            return result;
        }
        if (text.length() > 0 && text.charAt(0) == '\uFEFF') {
            text = text.substring(1);
        }

        String[] raw = text.split("\r?\n");
        String pendingName = null;
        String pendingId = "";
        String pendingGroup = "";
        String pendingRes = null;
        String txtGroup = "";

        for (String rawLine : raw) {
            String l = rawLine == null ? "" : rawLine.trim();
            if (l.length() == 0) {
                continue;
            }

            if (l.startsWith("#EXTINF")) {
                int comma = l.lastIndexOf(',');
                String rest = comma >= 0 ? l.substring(comma + 1).trim() : "";
                String resAttr = attr(l, "resolution");
                if (resAttr.length() == 0) {
                    Matcher m = Pattern.compile("\\[(\\d+p)[^\\]]*\\]", Pattern.CASE_INSENSITIVE).matcher(rest);
                    if (m.find()) {
                        resAttr = m.group(1);
                    }
                }
                String cleaned = rest.replaceAll("\\s*\\[[^\\]]*\\]", "").trim();
                if (cleaned.length() == 0) {
                    cleaned = attr(l, "tvg-name");
                }
                pendingName = cleaned.length() == 0 ? "未命名频道" : cleaned;
                pendingId = attr(l, "tvg-id");
                pendingGroup = attr(l, "group-title");
                pendingRes = resAttr.length() == 0 ? null : resAttr.toUpperCase();
                continue;
            }

            if (l.startsWith("#")) {
                continue;
            }

            if (l.endsWith(",#genre#")) {
                txtGroup = l.substring(0, l.length() - 8).trim();
                continue;
            }

            boolean isUrl = startsWithHttp(l);

            if (pendingName == null && l.contains(",") && !isUrl) {
                int split = l.indexOf(',');
                String name = l.substring(0, split).trim();
                String url = l.substring(split + 1).trim();
                if (startsWithHttp(url)) {
                    Channel c = new Channel(name, url);
                    c.group = classifyGroup(name, txtGroup);
                    c.source = sourceName;
                    result.add(c);
                }
                continue;
            }

            if (isUrl) {
                Channel c = new Channel();
                c.name = pendingName == null ? ("频道 " + (result.size() + 1)) : pendingName;
                c.id = (pendingId == null || pendingId.length() == 0) ? c.name : pendingId;
                c.group = classifyGroup(c.name, pendingGroup.length() > 0 ? pendingGroup : txtGroup);
                c.url = l;
                c.lines.add(l);
                c.source = sourceName;
                if (pendingRes != null) {
                    c.resolution = pendingRes;
                    Matcher m = Pattern.compile("(\\d+)").matcher(pendingRes);
                    if (m.find()) {
                        c.height = parseIntSafe(m.group(1));
                    }
                }
                result.add(c);
                pendingName = null;
                pendingId = "";
                pendingGroup = "";
                pendingRes = null;
            }
        }
        return result;
    }

    /** Merges duplicates (same station) and accumulates their mirror URLs into `lines`. */
    public static List<Channel> merge(List<Channel> existing, List<Channel> incoming) {
        Map<String, Channel> map = new LinkedHashMap<String, Channel>();
        addAll(map, existing);
        addAll(map, incoming);
        return new ArrayList<Channel>(map.values());
    }

    private static void addAll(Map<String, Channel> map, List<Channel> list) {
        if (list == null) {
            return;
        }
        for (Channel item : list) {
            if (item == null || item.url == null || item.url.length() == 0) {
                continue;
            }
            if (item.lines.isEmpty()) {
                item.lines.add(item.url);
            }
            String key = item.key();
            Channel prev = map.get(key);
            if (prev == null) {
                map.put(key, item);
                continue;
            }
            for (String nl : item.lines) {
                if (!prev.lines.contains(nl)) {
                    prev.lines.add(nl);
                }
            }
            if (prev.resolution.length() == 0 && item.resolution.length() > 0) {
                prev.resolution = item.resolution;
                prev.height = item.height;
            }
            if (prev.source.length() > 0 && item.source.length() > 0) {
                if (!prev.source.contains(item.source)) {
                    prev.source = prev.source + ", " + item.source;
                }
            } else if (prev.source.length() == 0) {
                prev.source = item.source;
            }
            if (item.latency >= 0 && (prev.latency < 0 || item.latency < prev.latency)) {
                prev.latency = item.latency;
            }
        }
    }

    private static boolean startsWithHttp(String s) {
        String lower = s.toLowerCase();
        return lower.startsWith("http://") || lower.startsWith("https://");
    }

    private static String attr(String line, String key) {
        Matcher m = Pattern.compile(key + "=\"([^\"]*)\"", Pattern.CASE_INSENSITIVE).matcher(line);
        return m.find() ? m.group(1) : "";
    }

    private static int parseIntSafe(String s) {
        try {
            return Integer.parseInt(s);
        } catch (NumberFormatException e) {
            return 0;
        }
    }
}
