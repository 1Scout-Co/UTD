// Sinh bởi: node ~/.claude/hooks/arch.mjs gen (từ docs/arch.json) — sửa docs/arch.json rồi chạy lại, đừng sửa tay file này.
import path from 'node:path'

/** @type {import('dependency-cruiser').IConfiguration} */
export default {
  "forbidden": [
    {
      "name": "no-circular",
      "severity": "error",
      "comment": "Hai file import vòng tròn nhau. Cách sửa: tách phần dùng chung của chúng ra file thứ ba, rồi cả hai cùng import file đó.",
      "from": {},
      "to": {
        "circular": true
      }
    },
    {
      "name": "not-to-unresolvable",
      "severity": "error",
      "comment": "Import một file hoặc gói không tìm thấy. Cách sửa: sửa lại đường dẫn import, hoặc cài gói còn thiếu bằng npm i.",
      "from": {},
      "to": {
        "couldNotResolve": true,
        "pathNot": [
          "^virtual:",
          "^cloudflare:",
          "^astro:",
          "^node:",
          "^bun:",
          "\\?(raw|url|inline|worker)$"
        ]
      }
    },
    {
      "name": "arch-web-components",
      "severity": "error",
      "comment": "Module \"components\" (src/components) không được import \"features/home\". Chỉ được dùng: lib, shared (docs/arch.json: mayDependOn). Cách sửa: chuyển phần dùng chung xuống shared, hoặc đi qua hàm/sự kiện của module kia thay vì import thẳng; nếu quan hệ này thật sự cần thì sửa mayDependOn trong docs/arch.json — phải hỏi người dùng trước.",
      "from": {
        "path": "^src/components/"
      },
      "to": {
        "path": "^(src/features/home)/"
      }
    },
    {
      "name": "arch-web-lib",
      "severity": "error",
      "comment": "Module \"lib\" (src/lib) không được import \"features/home\". Chỉ được dùng: components, shared (docs/arch.json: mayDependOn). Cách sửa: chuyển phần dùng chung xuống shared, hoặc đi qua hàm/sự kiện của module kia thay vì import thẳng; nếu quan hệ này thật sự cần thì sửa mayDependOn trong docs/arch.json — phải hỏi người dùng trước.",
      "from": {
        "path": "^src/lib/"
      },
      "to": {
        "path": "^(src/features/home)/"
      }
    },
    {
      "name": "arch-web-shared",
      "severity": "error",
      "comment": "Module \"shared\" (src/shared) không được import \"features/home\". Chỉ được dùng: components, lib (docs/arch.json: mayDependOn). Cách sửa: chuyển phần dùng chung xuống shared, hoặc đi qua hàm/sự kiện của module kia thay vì import thẳng; nếu quan hệ này thật sự cần thì sửa mayDependOn trong docs/arch.json — phải hỏi người dùng trước.",
      "from": {
        "path": "^src/shared/"
      },
      "to": {
        "path": "^(src/features/home)/"
      }
    },
    {
      "name": "arch-web-cam-1",
      "severity": "error",
      "comment": "Phần dữ liệu và gọi máy chủ không được biết giao diện. Cách sửa: Chuyển phần dùng chung sang model hoặc shared; ui mới là bên gọi model/api, không ngược lại.",
      "from": {
        "path": "^(?:src/features/[^/]*/model|src/features/[^/]*/api)(?:/|$)"
      },
      "to": {
        "path": "^(?:src/features/[^/]*/ui)(?:/|$)"
      }
    },
    {
      "name": "arch-web-cam-2",
      "severity": "error",
      "comment": "Gọi dịch vụ ngoài (Supabase, Stripe…) chỉ được làm trong api của tính năng hoặc shared/lib. Cách sửa: Thêm một hàm trong features/<tên>/api (hoặc shared/lib) bọc lời gọi, rồi gọi hàm đó.",
      "from": {
        "path": "^(?:src/features/[^/]*/ui|src/features/[^/]*/model)(?:/|$)"
      },
      "to": {
        "path": "(?:^|/)(?:node_modules/@supabase|node_modules/@stripe|node_modules/stripe)(?:/|$)"
      }
    },
    {
      "name": "arch-web-cam-3",
      "severity": "error",
      "comment": "shared không được biết tính năng nào, nếu không các tính năng sẽ dính nhau qua shared. Cách sửa: Chuyển đoạn code đó vào đúng tính năng, hoặc để tính năng truyền dữ liệu vào shared qua tham số.",
      "from": {
        "path": "^(?:src/shared)(?:/|$)"
      },
      "to": {
        "path": "^(?:src/features)(?:/|$)"
      }
    }
  ],
  "options": {
    "doNotFollow": {
      "path": "node_modules"
    },
    "tsPreCompilationDeps": true,
    "enhancedResolveOptions": {
      "exportsFields": [
        "exports"
      ],
      "conditionNames": [
        "import",
        "require",
        "node",
        "default",
        "types"
      ]
    },
    "tsConfig": {
      "fileName": path.join(import.meta.dirname, "tsconfig.json")
    }
  }
}
