// 错误码 → 用户可读中文文案（与 Rust `ErrorCode` 一一对应）。

import type { ErrorCode } from './AppError'

export const ERROR_MESSAGES: Record<ErrorCode, string> = {
  InvalidPetId: '宠物 ID 不合法',
  ZipSlip: '压缩包路径非法（疑似路径穿越）',
  TooLarge: '文件过大，导入失败',
  BadRequest: '请求格式错误',
  Network: '网络请求失败，请检查网络连接',
  Io: '文件读写失败',
  Platform: '当前平台不支持该操作',
  Serialization: '数据解析失败',
  Unknown: '发生未知错误',
}

/** 按错误码取文案；未知码回退到 Unknown。 */
export function errorMessage(code: ErrorCode): string {
  return ERROR_MESSAGES[code] ?? ERROR_MESSAGES.Unknown
}
