import { Routes, Route, Navigate } from 'react-router-dom';
import { Layout } from './components/Layout';
import { LoginPage } from './pages/LoginPage';
import { DashboardPage } from './pages/DashboardPage';
import { DagsPage } from './pages/DagsPage';
import { DagDetailPage } from './pages/DagDetailPage';
import { RunDetailPage } from './pages/RunDetailPage';
import { RunsPage } from './pages/RunsPage';
import { CompliancePage } from './pages/CompliancePage';
import { RBACPage } from './pages/RBACPage';
import { MonitoringPage } from './pages/MonitoringPage';
import { SettingsPage } from './pages/SettingsPage';
import { SwarmPage } from './pages/SwarmPage';
import { LineagePage } from './pages/LineagePage';
import { ConnectorsPage } from './pages/ConnectorsPage';
import { EventsPage } from './pages/EventsPage';

function PrivateRoute({ children }: { children: React.ReactNode }) {
  const token = localStorage.getItem('vortex_token');
  return token ? <>{children}</> : <Navigate to="/login" replace />;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        element={
          <PrivateRoute>
            <Layout />
          </PrivateRoute>
        }
      >
        <Route path="/" element={<DashboardPage />} />
        <Route path="/dags" element={<DagsPage />} />
        {/* Run detail must come before :dagId to avoid route capture */}
        <Route path="/dags/:dagId/runs/:runId" element={<RunDetailPage />} />
        <Route path="/dags/:dagId" element={<DagDetailPage />} />
        <Route path="/runs" element={<RunsPage />} />
        <Route path="/compliance" element={<CompliancePage />} />
        <Route path="/rbac" element={<RBACPage />} />
        <Route path="/monitoring" element={<MonitoringPage />} />
        <Route path="/swarm" element={<SwarmPage />} />
        <Route path="/lineage" element={<LineagePage />} />
        <Route path="/connectors" element={<ConnectorsPage />} />
        <Route path="/events" element={<EventsPage />} />
        <Route path="/settings" element={<SettingsPage />} />
      </Route>
    </Routes>
  );
}
