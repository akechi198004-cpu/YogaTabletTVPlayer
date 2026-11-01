export interface SubscriptionSource {
  id: string;
  title: string;
  shortTitle: string;
  url: string;
  desc: string;
  badge?: string;
  isDefault?: boolean;
}

export const PRESET_SOURCES: SubscriptionSource[] = [
  {
    id: 'default',
    title: '⚡ 360P-720P 极速优选 (系统内置)',
    shortTitle: '360-720P 优选',
    url: '/default-sources.m3u',
    desc: '经全网实测，严格筛选360P-720P低延迟高可用源 (44频道)',
    badge: '内置优选',
    isDefault: true,
  },
  {
    id: 'iptv-org-all',
    title: '🌐 iptv-org 全球全量源 (10,000+ 频道)',
    shortTitle: 'iptv-org 全量',
    url: 'https://iptv-org.github.io/iptv/index.m3u',
    desc: '全球公开电视台全量索引源，已针对 Chromebook 优化分页与播放',
    badge: '全球全量',
  },
  {
    id: 'iptv-org-cn',
    title: '🇨🇳 中国公开台 (iptv-org cn.m3u)',
    shortTitle: '中国公开台',
    url: 'https://iptv-org.github.io/iptv/countries/cn.m3u',
    desc: 'iptv-org 官方维护中国公共电视频道',
    badge: '公共中文',
  },
  {
    id: 'iptv-org-zho',
    title: '中文综合频道 (iptv-org zho.m3u)',
    shortTitle: '中文综合台',
    url: 'https://iptv-org.github.io/iptv/languages/zho.m3u',
    desc: '全球中文广播电视频道聚合',
    badge: '中文聚合',
  },
  {
    id: 'iptv-org-jp',
    title: '🇯🇵 日本精选电视台 (iptv-org jp.m3u)',
    shortTitle: '日本公开台',
    url: 'https://iptv-org.github.io/iptv/countries/jp.m3u',
    desc: '日本公开广播电视频道',
    badge: '日韩精选',
  },
  {
    id: 'all',
    title: '🌈 全部已载入订阅 (全量视图)',
    shortTitle: '全量合并',
    url: 'ALL',
    desc: '展示当前所有已载入订阅源的去重合并列表',
    badge: '合并视图',
  },
];
