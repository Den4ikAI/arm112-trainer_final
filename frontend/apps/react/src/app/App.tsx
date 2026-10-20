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
import { LessonsPage } from '@/pages/lessons/LessonsPage';
import { WeightsPage } from '@/pages/teacher/WeightsPage';
import { PhonebookPage } from '@/pages/teacher/PhonebookPage';
import { DemoStatsPage } from '@/pages/teacher/DemoStatsPage';
import { LessonMonitorPage } from '@/pages/teacher/LessonMonitorPage';
import { useSession, otherDuty, dutyTitle } from '@/entities/session/model';
import { NotForDuty } from '@/shared/ui/NotForDuty';
import { useCatalog } from '@/entities/catalog/model';
import { useStand } from '@/entities/stand/model';

/**
 * Маршруты повторяют пути боевой ПОВ-112 (/#!/incident/<номер>),
 * плюс учебный экран разбора, которого в боевой системе нет.
 */
export const App = () => {
  const { user, checking, restore } = useSession();
  const loadCatalog = useCatalog((st) => st.load);
  const loadStand = useStand((st) => st.load);
  const standLoaded = useStand((st) => st.loaded);

  // Сохранённый токен проверяем один раз при запуске: перезагрузка
  // страницы посреди занятия не должна выкидывать на экран входа
  useEffect(() => { void restore(); }, [restore]);

  // Сведения о стенде нужны уже экрану входа: учебные учётные записи
  // и свёрнут ли приём вызовов 112
  useEffect(() => { void loadStand(); }, [loadStand]);

  // Справочники нужны всем экранам, кроме входа
  useEffect(() => { if (user) void loadCatalog(); }, [user, loadCatalog]);

  // Свёрнут ли 112, экраны решают при первом показе (вкладка, режим
  // занятия): до ответа о стенде их не рисуем, иначе после перезагрузки
  // страница открывалась как при свёрнутом 112
  if (checking || !standLoaded) return <div data-testid="app-loading" />;

  return (
    <Routes>
      <Route path="/" element={<Navigate to={user ? '/journal' : '/login'} replace />} />
      <Route path="/login" element={user ? <Navigate to="/journal" replace /> : <LoginPage />} />
      <Route path="/journal" element={<Private><JournalPage /></Private>} />
      <Route path="/card/:number" element={<Private><Op112><Card112Page /></Op112></Private>} />
      <Route path="/card-dds/:id" element={<Private><CardDdsPage /></Private>} />
      <Route path="/debrief" element={<Private><Op112><DebriefPage /></Op112></Private>} />
      <Route path="/admin" element={<Private><AdminPage /></Private>} />
      <Route path="/analytics" element={<Private><AnalyticsPage /></Private>} />
      <Route path="/scenarios" element={<Private><ScenariosPage /></Private>} />
      <Route path="/lessons" element={<Private><LessonsPage /></Private>} />
      <Route path="/lessons/:id/monitor" element={<Private><LessonMonitorPage /></Private>} />
      <Route path="/weights" element={<Private><WeightsPage /></Private>} />
      <Route path="/phonebook" element={<Private><PhonebookPage /></Private>} />
      <Route path="/demo-stats" element={<Private><DemoStatsPage /></Private>} />
    </Routes>
  );
};

/** Приём вызова 112 и его разбор — рабочее место оператора 112. */
const Op112 = ({ children }: { children: React.ReactNode }) => {
  const user = useSession((st) => st.user);
  if (otherDuty(user, 'op112')) {
    return <NotForDuty yours={dutyTitle(user?.duty ?? null).toLowerCase()}
                       what="Приём вызова и заполнение карточки 112 — работа оператора 112.
                             Диспетчер ДДС получает готовую карточку в ленту своей службы." />;
  }
  return <>{children}</>;
};

/** Без входа внутрь не пускаем. Сервер всё равно ответит 401, но
 *  показывать пустые экраны вместо формы входа незачем. */
const Private = ({ children }: { children: React.ReactNode }) => {
  const user = useSession((st) => st.user);
  const { pathname } = useLocation();
  if (!user) return <Navigate to="/login" state={{ from: pathname }} replace />;
  return <>{children}</>;
};
