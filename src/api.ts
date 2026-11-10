import { invoke } from "@tauri-apps/api/core";

export interface Session {
  id: number;
  started_at: number; // unix ms
  ended_at: number | null;
  frame_count: number;
}

export const startSession = () => invoke<Session>("start_session");
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
