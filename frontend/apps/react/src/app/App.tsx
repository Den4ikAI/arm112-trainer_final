import { useEffect } from 'react';
import { Navigate, Route, Routes, useLocation } from 'react-router-dom';
import { LoginPage } from '@/pages/login/LoginPage';
import { JournalPage } from '@/pages/journal/JournalPage';
import { Card112Page } from '@/pages/card112/Card112Page';
import { CardDdsPage } from '@/pages/card-dds/CardDdsPage';
import { DebriefPage } from '@/pages/debrief/DebriefPage';
import { AdminPage } from '@/pages/admin/AdminPage';
import { AnalyticsPage } from '@/pages/analytics/AnalyticsPage';
import { ScenariosPage } from '@/pages/scenarios/ScenariosPage';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';

/**
 * Маршруты повторяют пути боевой ПОВ-112 (/#!/incident/<номер>),
 * плюс учебный экран разбора, которого в боевой системе нет.
 */
export const App = () => {
  const { user, checking, restore } = useSession();
  const loadCatalog = useCatalog((st) => st.load);

  // Сохранённый токен проверяем один раз при запуске: перезагрузка
  // страницы посреди занятия не должна выкидывать на экран входа
  useEffect(() => { void restore(); }, [restore]);

  // Справочники нужны всем экранам, кроме входа
  useEffect(() => { if (user) void loadCatalog(); }, [user, loadCatalog]);

  if (checking) return <div data-testid="app-loading" />;

  return (
    <Routes>
      <Route path="/" element={<Navigate to={user ? '/journal' : '/login'} replace />} />
      <Route path="/login" element={user ? <Navigate to="/journal" replace /> : <LoginPage />} />
      <Route path="/journal" element={<Private><JournalPage /></Private>} />
      <Route path="/card/:number" element={<Private><Card112Page /></Private>} />
      <Route path="/card-dds/:id" element={<Private><CardDdsPage /></Private>} />
      <Route path="/debrief" element={<Private><DebriefPage /></Private>} />
      <Route path="/admin" element={<Private><AdminPage /></Private>} />
      <Route path="/analytics" element={<Private><AnalyticsPage /></Private>} />
      <Route path="/scenarios" element={<Private><ScenariosPage /></Private>} />
    </Routes>
  );
};

/** Без входа внутрь не пускаем. Сервер всё равно ответит 401, но
 *  показывать пустые экраны вместо формы входа незачем. */
const Private = ({ children }: { children: React.ReactNode }) => {
  const user = useSession((st) => st.user);
  const { pathname } = useLocation();
  if (!user) return <Navigate to="/login" state={{ from: pathname }} replace />;
  return <>{children}</>;
};
