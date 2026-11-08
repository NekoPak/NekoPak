import {afterEach,expect,it,vi} from 'vitest';
import {parseShareInput,resolveSharedEntry} from '../../src/pages/Shares';
import {api} from '../../src/lib/api';
afterEach(()=>vi.restoreAllMocks());
it('parses only known HTTPS share pages or a literal share id',()=>{
  expect(parseShareInput('https://mypikpak.com/s/ABC_def-123')).toBe('ABC_def-123');
  expect(parseShareInput('ABC_def-123')).toBe('ABC_def-123');
  for(const input of ['https://evil.example/s/ABC123','http://mypikpak.com/s/ABC123','https://u:p@mypikpak.com/s/ABC123','https://mypikpak.com/drive','abc/../../secret'])expect(parseShareInput(input)).toBeNull();
});

it('uses only the source handle minted by Rust and never substitutes raw shared IDs',async()=>{
  const entry={id:'raw-file',name:'clip.mp4',kind:'file' as const,size_bytes:'100'};
  const encoded={...entry,id:'shared:opaque:minted'};
  const mint=vi.spyOn(api,'sharePreviewEntry').mockResolvedValue(encoded);
  expect(await resolveSharedEntry('opaque',entry)).toBe(encoded);
  expect(mint).toHaveBeenCalledWith('opaque','raw-file');
  mint.mockResolvedValue(entry);
  await expect(resolveSharedEntry('opaque',entry)).rejects.toMatchObject({code:'INTEGRITY_FAILED'});
});
