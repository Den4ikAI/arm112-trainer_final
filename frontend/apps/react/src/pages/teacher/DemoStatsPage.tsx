import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { DemoStatsPanel } from '@/widgets/demo-stats/DemoStatsPanel';
import s from './Teacher.module.scss';

/** Демо-статистика — отдельный экран преподавателя и администратора. */
export const DemoStatsPage = () => (
  <div className={s.page}>
    <header className={s.head}>
      <div>
        <h1 className={s.title}>Демо-статистика</h1>
        <p className={s.sub}>Заполнить программу историей обучения или вернуть её к чистому виду.</p>
      </div>
      <div className={s.tools}>
        <UserMenu />
      </div>
    </header>
    <DemoStatsPanel />
  </div>
);
