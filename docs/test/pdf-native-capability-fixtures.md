# PDF 原生能力 fixture 清单

本清单冻结阶段 0 可复核的仓库内 PDF 样本。哈希用于防止测试样本被静默替换；样本不等同于阶段 6 的质量集合。

| fixture | SHA-256 | 用途 | 当前可验证项 |
| --- | --- | --- | --- |
| `benchmarks/fixtures/pdf/normal-1-page.pdf` | `c248518d126bd5634b91d3d3238d68235c33a9690bd459ed9eed9ae0e9d835df` | 单页基础输入 | 页数、页面几何、原生文字提取路径 |
| `benchmarks/fixtures/pdf/normal-100-pages.pdf` | `2935510b108a68f1cde90a4db1471988b2d62af10ddfd366c79076c7ef5a2971` | 生命周期/逐页分析 | 页数、分页和对象释放路径 |
| `benchmarks/fixtures/pdf/complex-20-pages.pdf` | `391b4f1a24e984c21a638f07af68019a50c62bc08a797eca21c64c3bac75a17d` | 复杂页面回归 | 页面几何和分类回归 |
| `benchmarks/fixtures/pdf/varied-page-sizes.pdf` | `7bdd586be8dd909ef0148b77a010f705d46794732e9ce9ad9730c01b4bc80929` | 几何归一化 | 页面尺寸与 bbox 归一化 |

阶段 0 仍需补充专门的“纯原生文字 / 扫描图 / 隐藏文字层”三页 fixture，不能用上述通用样本替代该验收项。
