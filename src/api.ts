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
}

export const captureStatus = () => invoke<CaptureStatus>("capture_status");
