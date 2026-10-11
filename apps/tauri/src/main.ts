import { invoke } from "@tauri-apps/api/core";

type Envelope<T> = {
  schema_version: string;
  request_id: string;
  ok: boolean;
  result?: T;
  error?: { code: string; message: string; retryable: boolean };
};

const stateElement = document.querySelector<HTMLPreElement>("#state");
const refreshButton = document.querySelector<HTMLButtonElement>("#refresh");

async function refreshState(): Promise<void> {
  if (!stateElement) return;
  stateElement.textContent = "正在读取…";
  try {
    const response = await invoke<Envelope<unknown>>("get_environment_state");
    stateElement.textContent = JSON.stringify(response, null, 2);
  } catch (error) {
    stateElement.textContent = `桌面宿主不可用：${String(error)}`;
  }
}

refreshButton?.addEventListener("click", () => void refreshState());
void refreshState();
