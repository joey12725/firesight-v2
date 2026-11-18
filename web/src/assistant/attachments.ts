// Files the user attaches for the assistant to read: images and PDFs of pay stubs,
// account, mortgage and Social Security statements. They are read in this browser and
// sent only as part of the model request; nothing is uploaded anywhere else.
import type { ApiContent, TransportMode } from './agent/types';

export const IMAGE_TYPES = ['image/jpeg', 'image/png', 'image/gif', 'image/webp'] as const;
export const PDF_TYPE = 'application/pdf';
export const ACCEPT = [...IMAGE_TYPES, PDF_TYPE].join(',');

export interface PreparedAttachment {
  name: string;
  mediaType: (typeof IMAGE_TYPES)[number] | typeof PDF_TYPE;
  bytes: number;
  /** Base64, no line breaks. */
  data: string;
}

export const MAX_FILES = 6;
/** The API takes images up to 5 MB each. */
const MAX_IMAGE_BYTES = 5_000_000;
/** Per message. Direct requests may be 32 MB; the hosted proxy sits behind a 4.5 MB platform limit. */
export const MAX_TOTAL_BYTES: Record<TransportMode, number> = { byok: 20_000_000, hosted: 2_500_000 };

export function isSupported(type: string): type is PreparedAttachment['mediaType'] {
  return type === PDF_TYPE || (IMAGE_TYPES as readonly string[]).includes(type);
}

function toBase64(bytes: Uint8Array): string {
  let binary = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(binary);
}

export type AttachmentProblem = { name: string; reason: string };

/** Reads files into attachments, reporting the ones that cannot be sent and why. */
export async function prepareAttachments(
  files: File[],
  existing: PreparedAttachment[],
  mode: TransportMode,
): Promise<{ added: PreparedAttachment[]; problems: AttachmentProblem[] }> {
  const added: PreparedAttachment[] = [];
  const problems: AttachmentProblem[] = [];
  let total = existing.reduce((n, a) => n + a.bytes, 0);
  for (const file of files) {
    if (existing.length + added.length >= MAX_FILES) {
      problems.push({ name: file.name, reason: `At most ${MAX_FILES} files per message.` });
      continue;
    }
    if (!isSupported(file.type)) {
      problems.push({ name: file.name, reason: 'Only images (JPEG, PNG, GIF, WebP) and PDFs can be read.' });
      continue;
    }
    if (file.type !== PDF_TYPE && file.size > MAX_IMAGE_BYTES) {
      problems.push({ name: file.name, reason: 'Images can be up to 5 MB each.' });
      continue;
    }
    if (total + file.size > MAX_TOTAL_BYTES[mode]) {
      problems.push({
        name: file.name,
        reason:
          mode === 'hosted'
            ? 'The hosted assistant accepts about 2.5 MB of attachments per message. Use a smaller file or a screenshot of the relevant page, or add your own API key for larger files.'
            : 'Attachments can total 20 MB per message.',
      });
      continue;
    }
    total += file.size;
    added.push({ name: file.name, mediaType: file.type, bytes: file.size, data: toBase64(new Uint8Array(await file.arrayBuffer())) });
  }
  return { added, problems };
}

/** Content blocks for the request. Documents and images go before the text that refers to them. */
export function attachmentBlocks(attachments: PreparedAttachment[]): ApiContent[] {
  return attachments.map((a): ApiContent =>
    a.mediaType === PDF_TYPE
      ? { type: 'document', source: { type: 'base64', media_type: PDF_TYPE, data: a.data }, title: a.name }
      : { type: 'image', source: { type: 'base64', media_type: a.mediaType, data: a.data } },
  );
}
