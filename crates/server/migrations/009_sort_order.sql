-- 手动排序：后台拖拽调整主机顺序（仪表盘卡片顺序同步）
-- 0 = 未调整过，与 created_at 一起保持稳定序
ALTER TABLE servers ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
