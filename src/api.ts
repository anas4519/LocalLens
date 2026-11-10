import { invoke } from "@tauri-apps/api/core";

export interface Session {
  id: number;
  name: string | null; // null = unnamed; UI shows date + time either way
  started_at: number; // unix ms
  ended_at: number | null;
  frame_count: number;
}

export const MAX_SESSION_NAME = 80; // mirrors db::MAX_SESSION_NAME

export const startSession = (name?: string | null) =>
  invoke<Session>("start_session", { name: name?.trim() || null });
export const renameSession = (id: number, name: string | null) =>
  invoke<Session>("rename_session", { id, name });
export const stopSession = () => invoke<Session>("stop_session");
export const listSessions = () => invoke<Session[]>("list_sessions");

export interface CaptureStatus {
  has_permission: boolean;
  active: Session | null;
  capture_error: string | null;
  ocr_pending: number;
}

export const captureStatus = () => invoke<CaptureStatus>("capture_status");

export const OCR_PENDING = 0;
export const OCR_DONE = 1;
export const OCR_FAILED = 2;

export interface FrameRow {
  id: number;
  ts: number; // unix ms
  image_path: string | null; // absolute; null when pruned
  ocr_text: string;
  ocr_status: number;
}

export const listFrames = (sessionId: number) => invoke<FrameRow[]>("list_frames", { sessionId });
