import { z } from 'zod';
import { PROTOCOL_VERSION } from './version.ts';

export const requestSchema = z.object({
  protocolVersion: z.literal(PROTOCOL_VERSION),
  requestId: z.string().uuid(),
  method: z.string().min(1).max(100),
  params: z.unknown().optional(),
});
export type RpcRequest = z.infer<typeof requestSchema>;
export const responseSchema = z.object({
  protocolVersion: z.literal(PROTOCOL_VERSION),
  requestId: z.string().uuid(),
  result: z.unknown().optional(),
  error: z.object({ code: z.string(), message: z.string() }).optional(),
});
