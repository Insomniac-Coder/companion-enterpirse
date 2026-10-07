import React from 'react';
import { createRoot } from 'react-dom/client';
import SignInGate from '../SignInGate';
import AdminApp from './AdminApp';
import '../workbench.css';
import '../pages.css';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <SignInGate app={AdminApp} />
  </React.StrictMode>,
);
