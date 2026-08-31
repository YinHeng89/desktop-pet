// 统一应用错误类型（与 Rust `domain::error::AppError` / `ErrorCode` 一一对应）。
//
// 前端 invoke 失败时统一包装成 AppError，UI 按 `code` 映射中文文案（messages.ts），
// 而非裸吞异常（R7 错误不吞）。

export type ErrorCode =
  | 'InvalidPetId'
  | 'ZipSlip'
  | 'TooLarge'
  | 'BadRequest'
  | 'Network'
  | 'Io'
  | 'Platform'
  | 'Serialization'
  | 'Unknown'

export class AppError extends Error {
  code: ErrorCode
  override cause?: unknown

  constructor(code: ErrorCode, message: string, cause?: unknown) {
    super(message)
    this.name = 'AppError'
    this.code = code
    this.cause = cause
  }
}
