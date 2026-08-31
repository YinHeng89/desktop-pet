// 宠物测试数据夹具
//
// 说明：petStore 是模块级单例，但 vitest 默认按测试文件隔离模块注册表，
// 因此每个 spec 文件拿到的是独立实例；同一文件内多个用例共享，
// 需在 beforeEach 中调用 resetPetStore() 复位。

import { petStore, type PetDef } from '../../store/pet'

/** 构造一个字段完整的宠物定义（可按需覆盖） */
export function makePet(overrides: Partial<PetDef> = {}): PetDef {
  return {
    id: 'test-pet',
    displayName: '测试宠物',
    description: '用于单元测试',
    dir: 'test-pet',
    spritesheet: 'test/spritesheet.webp',
    idle: { row: 0, count: 6, fps: 8 },
    talk: { row: 3, count: 4, fps: 10 },
    actions: {
      wave: { row: 3, count: 4, fps: 10 },
      jump: { row: 4, count: 5, fps: 10 },
    },
    ...overrides,
  }
}

/** 清空宠物状态（用例间隔离用） */
export function resetPetStore(): void {
  petStore.pets = []
  petStore.currentId = ''
  petStore.frame = { width: 192, height: 208, cols: 8 }
  petStore.scale = 1
  petStore.visible = true
}

/** 用给定的宠物列表填充 store，并选中第一只 */
export function setupPetStore(pets: PetDef[] = [makePet()]): void {
  petStore.pets = pets
  petStore.currentId = pets[0]?.id ?? ''
  petStore.frame = { width: 192, height: 208, cols: 8 }
  petStore.scale = 1
  petStore.visible = true
}
