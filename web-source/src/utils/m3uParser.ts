/**
 * M3U Parser and Channel Classification Utilities
 */

export interface ChannelItem {
  name: string;
  id: string;
  logo?: string;
  group: string;
  url: string;
  lines: string[];
  resolution?: string;
  height?: number;
  latency?: number;
  source?: string;
  seq?: number;
}

export function classifyGroup(name: string, group: string = ''): string {
  const KNOWN_GROUPS = ['央视', '卫视', '苏州', '香港', '台湾', '日本', '国际', '纪录', '少儿', '测试'];
  if (KNOWN_GROUPS.includes(group)) return group;
  const t = (name + ' ' + group).toLowerCase();
  if (/cctv|央视|中央/.test(t)) return '央视';
  if (/卫视|湖南|浙江|江苏|北京|东方|深圳|广东|山东|安徽|湖北|四川|天津|河南|江西|辽宁|河北|黑龙江|吉林|重庆|三沙|东南|厦门|宁夏|山西/.test(t)) return '卫视';
  if (/苏州/.test(t)) return '苏州';
  if (/tvb|翡翠|黄金华剧|美亚|凤凰|香港/.test(t)) return '香港';
  if (/大立|人间卫视|tvbs|台湾/.test(t)) return '台湾';
  if (/nhk|weathernews|mbs|日本/.test(t)) return '日本';
  if (/cnn|bbc news|fox|cna|nbc|dw|cnbc|sky news|scripps|brics|reuters|usa today|半岛|国际/.test(t)) return '国际';
  if (/earth|nature|documentary|纪录|cgtn documentary|wonder|hit/.test(t)) return '纪录';
  if (/少儿|卡通|animation|cartoon|cctv-?14/.test(t)) return '少儿';
  if (/体育|sport|espn|nba|五星体育|奥林匹克|高尔夫|cctv-?5|cctv-?16/.test(t)) return '体育';
  if (/电影|电视剧|影视|movie|cinema|剧场|影院|chc|怀旧|cctv-?6|cctv-?8/.test(t)) return '影视';
  if (/新闻|news|财经|business|社会与法|cctv-?13|cctv-?2/.test(t)) return '新闻';
  return group || '其他';
}

export function is360To720P(val?: number | string): boolean {
  if (!val) return false;
  if (typeof val === 'number') {
    return val >= 360 && val <= 720;
  }
  const match = String(val).match(/(\d+)/);
  if (!match) return false;
  const num = parseInt(match[1], 10);
  return num >= 360 && num <= 720;
}

export function parseM3u(text: string, sourceName: string = ''): ChannelItem[] {
  const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/);
  const result: ChannelItem[] = [];
  let meta: { name: string; id: string; logo: string; group: string; resolution?: string } | null = null;
  let txtGroup = '';

  for (const raw of lines) {
    const l = raw.trim();
    if (!l) continue;

    if (l.startsWith('#EXTINF')) {
      const rest = l.slice(l.lastIndexOf(',') + 1).trim();
      const attr = (k: string) => {
        const m = l.match(new RegExp(k + '="([^"]*)"', 'i'));
        return m ? m[1] : '';
      };
      const resAttr = attr('resolution') || (rest.match(/\[(\d+p)[^\]]*\]/i)?.[1] || '');
      meta = {
        name: rest.replace(/\s*\[[^\]]*\]/g, '').trim() || attr('tvg-name') || '未命名频道',
        id: attr('tvg-id'),
        logo: attr('tvg-logo'),
        group: attr('group-title'),
        resolution: resAttr ? resAttr.toUpperCase() : undefined,
      };
      continue;
    }

    if (l.startsWith('#')) continue;

    if (l.endsWith(',#genre#')) {
      txtGroup = l.slice(0, -8).trim();
      continue;
    }

    if (l.includes(',') && !meta) {
      const split = l.indexOf(',');
      const name = l.slice(0, split).trim();
      const url = l.slice(split + 1).trim();
      if (/^https?:\/\//i.test(url)) {
        result.push({
          name,
          id: name,
          logo: '',
          group: classifyGroup(name, txtGroup),
          url,
          lines: [url],
          source: sourceName,
        });
      }
      continue;
    }

    if (/^https?:\/\//i.test(l)) {
      const info = meta || { name: '频道 ' + (result.length + 1), id: '', logo: '', group: '' };
      let h: number | undefined;
      if (info.resolution) {
        const m = info.resolution.match(/(\d+)/);
        if (m) h = parseInt(m[1], 10);
      }
      result.push({
        name: info.name,
        id: info.id || info.name,
        logo: info.logo,
        group: classifyGroup(info.name, info.group || txtGroup),
        url: l,
        lines: [l],
        resolution: info.resolution,
        height: h,
        source: sourceName,
      });
      meta = null;
    }
  }

  return result;
}

export function mergeChannels(existing: ChannelItem[], incoming: ChannelItem[]): ChannelItem[] {
  const map = new Map<string, ChannelItem>();

  for (const item of [...existing, ...incoming]) {
    if (!item.url) continue;
    const key = (item.id && item.id.length > 3 ? item.id : item.name)
      .toLowerCase()
      .replace(/[\s\u00b7_\-()\[\]]/g, '');

    if (!map.has(key)) {
      map.set(key, {
        ...item,
        lines: item.lines && item.lines.length ? [...item.lines] : [item.url],
      });
    } else {
      const prev = map.get(key)!;
      const targetLines = prev.lines || [prev.url];
      const newLines = item.lines || [item.url];
      for (const nl of newLines) {
        if (!targetLines.includes(nl)) {
          targetLines.push(nl);
        }
      }
      prev.lines = targetLines;
      if (!prev.logo && item.logo) prev.logo = item.logo;
      if (!prev.resolution && item.resolution) prev.resolution = item.resolution;
      if (item.latency !== undefined && (prev.latency === undefined || item.latency < prev.latency)) {
        prev.latency = item.latency;
        prev.resolution = item.resolution || prev.resolution;
      }
      if (item.source && prev.source) {
        if (!prev.source.includes(item.source)) {
          prev.source = `${prev.source}, ${item.source}`;
        }
      } else if (item.source && !prev.source) {
        prev.source = item.source;
      }
    }
  }

  return Array.from(map.values());
}

export function filterChannels(
  channels: ChannelItem[],
  query: string,
  activeFilter: string,
  favorites: string[],
  activeSource?: string
): ChannelItem[] {
  const q = query.trim().toLowerCase();
  return channels.filter((c) => {
    // 1. Source filter (legacy compatibility if provided)
    if (activeSource && activeSource !== 'ALL') {
      if (activeSource === '/default-sources.m3u') {
        const isDefault =
          !c.source ||
          c.source.includes('default') ||
          c.source.includes('360-720P') ||
          c.source.includes('官方示例') ||
          c.source.includes('开源组合');
        if (!isDefault) return false;
      } else if (c.source) {
        if (!c.source.includes(activeSource)) return false;
      }
    }

    // 2. Query search
    const matchesQuery = !q || (c.name + ' ' + c.group + ' ' + (c.resolution || '')).toLowerCase().includes(q);
    if (!matchesQuery) return false;

    // 3. Category filter
    if (activeFilter === '全部') return true;
    if (activeFilter === '收藏') return favorites.includes(c.name);
    if (activeFilter === '优选360-720P') {
      return is360To720P(c.height || c.resolution);
    }
    return c.group === activeFilter;
  });
}

export function formatLatency(latency?: number): { text: string; status: 'fast' | 'medium' | 'slow' | 'error' | 'unknown' } {
  if (latency === -1) {
    return { text: '超时', status: 'error' };
  }
  if (latency === undefined || latency === null) {
    return { text: '--', status: 'unknown' };
  }
  if (latency <= 500) {
    return { text: `${latency}ms`, status: 'fast' };
  }
  if (latency <= 1200) {
    return { text: `${latency}ms`, status: 'medium' };
  }
  return { text: `${latency}ms`, status: 'slow' };
}
