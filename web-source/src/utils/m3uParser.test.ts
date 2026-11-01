import test from 'node:test';
import assert from 'node:assert/strict';
import {
  classifyGroup,
  is360To720P,
  parseM3u,
  mergeChannels,
  filterChannels,
  formatLatency,
} from './m3uParser.ts';

test('classifyGroup classifies CCTV channels as 央视', () => {
  assert.equal(classifyGroup('CCTV-1 综合'), '央视');
  assert.equal(classifyGroup('中央电视台中文国际'), '央视');
});

test('classifyGroup classifies satellite TV as 卫视', () => {
  assert.equal(classifyGroup('湖南卫视'), '卫视');
  assert.equal(classifyGroup('东方卫视'), '卫视');
  assert.equal(classifyGroup('广东卫视'), '卫视');
});

test('classifyGroup classifies regional and international groups', () => {
  assert.equal(classifyGroup('苏州新闻综合', '苏州'), '苏州');
  assert.equal(classifyGroup('TVB翡翠台', '香港'), '香港');
  assert.equal(classifyGroup('大立电视', '台湾'), '台湾');
  assert.equal(classifyGroup('NHK World', '日本'), '日本');
  assert.equal(classifyGroup('CNN', '国际'), '国际');
  assert.equal(classifyGroup('CGTN Documentary', '纪录'), '纪录');
  assert.equal(classifyGroup('金鹰卡通', '少儿'), '少儿');
});

test('classifyGroup classifies sports and movies', () => {
  assert.equal(classifyGroup('五星体育'), '体育');
  assert.equal(classifyGroup('欢笑剧场'), '影视');
  assert.equal(classifyGroup('CHC影院', '影视'), '影视');
});

test('is360To720P correctly validates height and resolution strings', () => {
  assert.equal(is360To720P(720), true);
  assert.equal(is360To720P(360), true);
  assert.equal(is360To720P(576), true);
  assert.equal(is360To720P('720P'), true);
  assert.equal(is360To720P('360p'), true);
  assert.equal(is360To720P('1080P'), false);
  assert.equal(is360To720P(1080), false);
  assert.equal(is360To720P(240), false);
  assert.equal(is360To720P(undefined), false);
});

test('parseM3u parses extended EXTINF and URL pairs', () => {
  const m3uSample = `#EXTM3U
#EXTINF:-1 tvg-name="CCTV-1" tvg-logo="https://logo.png" group-title="央视" resolution="720P",CCTV-1 综合 [720P · 150ms]
https://stream.example.com/cctv1.m3u8
#EXTINF:-1,广东卫视
https://stream.example.com/guangdong.m3u8
`;
  const parsed = parseM3u(m3uSample, 'test.m3u');
  assert.equal(parsed.length, 2);
  assert.equal(parsed[0].name, 'CCTV-1 综合');
  assert.equal(parsed[0].group, '央视');
  assert.equal(parsed[0].resolution, '720P');
  assert.equal(parsed[0].height, 720);
  assert.equal(parsed[0].url, 'https://stream.example.com/cctv1.m3u8');
  assert.equal(parsed[1].name, '广东卫视');
  assert.equal(parsed[1].group, '卫视');
});

test('mergeChannels consolidates backup lines and keeps optimal latency', () => {
  const list1 = [
    {
      name: 'CCTV-1',
      id: 'cctv1',
      group: '央视',
      url: 'https://stream1.m3u8',
      lines: ['https://stream1.m3u8'],
      latency: 500,
      resolution: '720P',
    },
  ];
  const list2 = [
    {
      name: 'CCTV-1',
      id: 'cctv1',
      group: '央视',
      url: 'https://stream2.m3u8',
      lines: ['https://stream2.m3u8'],
      latency: 200,
      resolution: '720P',
    },
  ];

  const merged = mergeChannels(list1, list2);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].lines.length, 2);
  assert.equal(merged[0].latency, 200); // picked lower latency
});

test('filterChannels works with search query, groups and favorites', () => {
  const items = [
    { name: 'CCTV-1 综合', id: '1', group: '央视', url: 'u1', lines: ['u1'], resolution: '720P', height: 720 },
    { name: '湖南卫视', id: '2', group: '卫视', url: 'u2', lines: ['u2'], resolution: '1080P', height: 1080 },
    { name: 'ABN 中文', id: '3', group: '其他', url: 'u3', lines: ['u3'], resolution: '360P', height: 360 },
  ];

  // Filter by group
  assert.equal(filterChannels(items, '', '央视', []).length, 1);

  // Filter by 360-720P
  const filtered720 = filterChannels(items, '', '优选360-720P', []);
  assert.equal(filtered720.length, 2); // CCTV-1 (720P) and ABN (360P)

  // Filter by search
  assert.equal(filterChannels(items, '湖南', '全部', []).length, 1);

  // Filter by favorites
  assert.equal(filterChannels(items, '', '收藏', ['湖南卫视']).length, 1);
});

test('formatLatency categorizes speed status', () => {
  assert.equal(formatLatency(150).status, 'fast');
  assert.equal(formatLatency(800).status, 'medium');
  assert.equal(formatLatency(1600).status, 'slow');
  assert.equal(formatLatency(-1).status, 'error');
  assert.equal(formatLatency(-1).text, '超时');
  assert.equal(formatLatency(undefined).status, 'unknown');
});
