import {expect,it} from 'vitest';
import {safeMediaSessionUrl,subtitleVtt} from '../../src/pages/Player';
it('accepts only an opaque loopback media session, never an upstream or credential URL',()=>{
  expect(safeMediaSessionUrl('http://127.0.0.1:49321/media/1234567890/movie')).toBe(true);
  for(const url of ['https://cdn.example/movie?token=secret','http://localhost:49321/media/1234567890','http://user:pass@127.0.0.1:49321/media/1234567890','file:///D:/movie.mp4'])expect(safeMediaSessionUrl(url)).toBe(false);
});
it('converts local SRT cue timing to VTT without interpreting markup as HTML',()=>{
  const text='1\r\n00:00:01,200 --> 00:00:03,400\r\n字幕😀 <b>safe text</b>\r\n';
  expect(subtitleVtt(text)).toBe('WEBVTT\n\n1\n00:00:01.200 --> 00:00:03.400\n字幕😀 <b>safe text</b>\n');
  expect(subtitleVtt('WEBVTT\n\n')).toBe('WEBVTT\n\n');
});
