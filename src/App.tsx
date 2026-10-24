import { DirectionProvider } from '@base-ui/react/direction-provider'

import { HomeScreen } from '@/features/home/ui/home-screen'
import { useLanguage } from '@/shared/i18n/i18n'
import { isRtl } from '@/shared/i18n/languages'

// Đổi ngôn ngữ ⇒ App vẽ lại cả cây (chữ mới), Base UI đổi hướng menu/phím mũi tên theo RTL.
export default function App() {
  const language = useLanguage()
  return (
    <DirectionProvider direction={isRtl(language) ? 'rtl' : 'ltr'}>
      <HomeScreen />
    </DirectionProvider>
  )
}
