// ArrayBuffer → Base64（从 PetSettings.vue 抽出，★ 零 Vue 依赖，可单测）。
// 外部宠物精灵图以 base64 data URL 存入 localStorage，导入时用到。

export function arrayBufferToBase64(buf: ArrayBuffer): string {
  let binary = ''
  const bytes = new Uint8Array(buf)
  const len = bytes.byteLength
  for (let i = 0; i < len; i++) binary += String.fromCharCode(bytes[i])
  return btoa(binary)
}
