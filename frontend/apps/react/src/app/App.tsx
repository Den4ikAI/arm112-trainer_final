import { Navigate, Route, Routes } from 'react-router-dom';
import { LoginPage } from '@/pages/login/LoginPage';
import { JournalPage } from '@/pages/journal/JournalPage';
import { Card112Page } from '@/pages/card112/Card112Page';
import { CardDdsPage } from '@/pages/card-dds/CardDdsPage';
import { DebriefPage } from '@/pages/debrief/DebriefPage';

/**
 * Маршруты повторяют пути боевой ПОВ-112 (/#!/incident/<номер>),
 * плюс учебный экран разбора, которого в боевой системе нет.
 */
export const App = () => (
  <Routes>
    <Route path="/" element={<Navigate to="/login" replace />} />
    <Route path="/login" element={<LoginPage />} />
    <Route path="/journal" element={<JournalPage />} />
    <Route path="/card/:number" element={<Card112Page />} />
    <Route path="/card-dds/:number" element={<CardDdsPage />} />
    <Route path="/debrief" element={<DebriefPage />} />
  </Routes>
);
